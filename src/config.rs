use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub hot_domain_detect: bool,
    pub enable_access_logs: bool,
    pub rate_limit: RateLimit,
    pub enable_priority_by_traffic: bool,
    pub priority_evaluate_interval_days: u64,
    pub priority_top_count: usize,
    pub high_traffic_threshold_requests_per_window: u64,
    pub enable_dp_session_api: bool,
    pub dashboard_password: String,
    pub dp_session_api_domain: String,
    pub dp_session_api_key: String,
    pub block_ai_bots: bool,
    pub prefer_default_static_pages: bool,
    pub maintenance_mode: bool,
    pub ip_whitelist: Vec<String>,
    pub discord_webhook: String,
    pub server_public_ip: String,
    pub enable_cloudflare: bool,
    pub version: String,
    pub cf_api_token: String,
    pub cf_zone_id: String,
    pub listen_address: String,
    pub listen_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RateLimit {
    pub limit: u64,
    pub window: u64,
}

impl Default for RateLimit {
    fn default() -> Self { Self { limit: 120, window: 60_000 } }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hot_domain_detect: true,
            enable_access_logs: true,
            rate_limit: RateLimit::default(),
            enable_priority_by_traffic: true,
            priority_evaluate_interval_days: 30,
            priority_top_count: 10,
            high_traffic_threshold_requests_per_window: 1000,
            enable_dp_session_api: false,
            dashboard_password: String::new(),
            dp_session_api_domain: String::new(),
            dp_session_api_key: String::new(),
            block_ai_bots: false,
            prefer_default_static_pages: true,
            maintenance_mode: false,
            ip_whitelist: vec![],
            discord_webhook: String::new(),
            server_public_ip: String::new(),
            enable_cloudflare: false,
            version: "v3.0.0".into(),
            cf_api_token: String::new(),
            cf_zone_id: String::new(),
            listen_address: "0.0.0.0".into(),
            listen_port: 80,
        }
    }
}

pub fn load() -> Result<Config> {
    let path = Path::new("config.json");
    if !path.exists() {
        let cfg = Config::default();
        save(&cfg)?;
        return Ok(cfg);
    }
    let raw = fs::read_to_string(path).context("config.json okunamadı")?;
    let cfg: Config = serde_json::from_str(&raw).context("config.json geçersiz JSON veya uyumsuz")?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let tmp = Path::new("config.json.tmp");
    fs::write(tmp, serde_json::to_vec_pretty(cfg)?)?;
    fs::rename(tmp, "config.json").context("config.json atomik olarak güncellenemedi")?;
    Ok(())
}

pub fn print_config(show: bool) -> Result<()> {
    let cfg = load()?;
    if show {
        println!("{}", serde_json::to_string_pretty(&cfg)?);
    } else {
        println!("config.json geçerli. Listen: {}:{}", cfg.listen_address, cfg.listen_port);
    }
    Ok(())
}

pub fn import_legacy(dir: &PathBuf) -> Result<()> {
    for name in ["config.json", "vhosts.json", "blacklist.json", "priority_sites.json", "visit_stats.json", "dpsessions.json"] {
        let source = dir.join(name);
        if source.exists() {
            let raw = fs::read_to_string(&source).with_context(|| format!("{} okunamadı", source.display()))?;
            let _: serde_json::Value = serde_json::from_str(&raw).with_context(|| format!("{} geçersiz JSON", source.display()))?;
            let target = Path::new(name);
            if !target.exists() {
                let tmp = target.with_extension("json.import.tmp");
                fs::write(&tmp, raw)?;
                fs::rename(&tmp, target)?;
                println!("İçe aktarıldı: {name}");
            } else {
                println!("Atlandı (zaten var): {name}");
            }
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_legacy_camel_case_config() {
        let cfg: Config = serde_json::from_str(r#"{
            "hotDomainDetect": false,
            "enableAccessLogs": true,
            "rateLimit": { "limit": 45, "window": 30000 },
            "ipWhitelist": ["127.0.0.1"],
            "maintenanceMode": true
        }"#).unwrap();

        assert!(!cfg.hot_domain_detect);
        assert_eq!(cfg.rate_limit.limit, 45);
        assert_eq!(cfg.rate_limit.window, 30000);
        assert_eq!(cfg.ip_whitelist, vec!["127.0.0.1"]);
        assert!(cfg.maintenance_mode);
        assert_eq!(cfg.listen_port, 80);
    }
}
