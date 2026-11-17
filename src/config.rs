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
    pub trusted_proxy_cidrs: Vec<String>,
    pub require_cloudflare: bool,
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
            trusted_proxy_cidrs: Vec::new(),
            require_cloudflare: true,
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
    replace_file(tmp, Path::new("config.json")).context("config.json güncellenemedi")?;
    Ok(())
}

pub fn print_config(show: bool) -> Result<()> {
    let cfg = load()?;
    if show {
        let mut value = serde_json::to_value(&cfg)?;
        for key in ["dashboardPassword", "dpSessionApiKey", "discordWebhook", "cfApiToken"] {
            if value.get(key).and_then(|item| item.as_str()).is_some_and(|secret| !secret.is_empty()) {
                value[key] = serde_json::Value::String("********".into());
            }
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("config.json geçerli. Listen: {}:{}", cfg.listen_address, cfg.listen_port);
    }
    Ok(())
}

pub fn set_value(key: &str, value: &str) -> Result<()> {
    let mut cfg = load()?;
    let parse_bool = || -> Result<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "on" | "yes" => Ok(true),
            "false" | "0" | "off" | "no" => Ok(false),
            _ => anyhow::bail!("Boolean değer true/false olmalı"),
        }
    };

    match key {
        "listenAddress" => {
            if value.trim().is_empty() { anyhow::bail!("listenAddress boş olamaz"); }
            cfg.listen_address = value.trim().to_string();
        }
        "listenPort" => {
            let port = value.parse::<u16>().context("listenPort 1-65535 arası sayı olmalı")?;
            if port == 0 { anyhow::bail!("listenPort 0 olamaz"); }
            cfg.listen_port = port;
        }
        "hotDomainDetect" => cfg.hot_domain_detect = parse_bool()?,
        "enableAccessLogs" => cfg.enable_access_logs = parse_bool()?,
        "enablePriorityByTraffic" => cfg.enable_priority_by_traffic = parse_bool()?,
        "enableDpSessionApi" => {
            let enabled = parse_bool()?;
            if enabled && (cfg.dp_session_api_key.trim().is_empty() || cfg.dp_session_api_domain.trim().is_empty()) {
                anyhow::bail!("Önce dpSessionApiDomain ve dpSessionApiKey ayarlamalısın");
            }
            cfg.enable_dp_session_api = enabled;
        }
        "blockAIBots" => cfg.block_ai_bots = parse_bool()?,
        "preferDefaultStaticPages" => cfg.prefer_default_static_pages = parse_bool()?,
        "maintenanceMode" => cfg.maintenance_mode = parse_bool()?,
        "enableCloudflare" => {
            let enabled = parse_bool()?;
            if enabled && (cfg.cf_api_token.trim().is_empty() || cfg.cf_zone_id.trim().is_empty()) {
                anyhow::bail!("Önce cfApiToken ve cfZoneId ayarlamalısın");
            }
            cfg.enable_cloudflare = enabled;
        }
        "dpSessionApiDomain" => cfg.dp_session_api_domain = value.trim().to_ascii_lowercase(),
        "dpSessionApiKey" => {
            if value.trim().is_empty() { anyhow::bail!("dpSessionApiKey boş bırakılamaz"); }
            cfg.dp_session_api_key = value.trim().to_string();
        }
        "discordWebhook" => cfg.discord_webhook = value.trim().to_string(),
        "cfApiToken" => cfg.cf_api_token = value.trim().to_string(),
        "cfZoneId" => cfg.cf_zone_id = value.trim().to_string(),
        "requireCloudflare" => cfg.require_cloudflare = parse_bool()?,
        "serverPublicIp" => {
            if !value.trim().is_empty() {
                value.trim().parse::<std::net::IpAddr>().context("serverPublicIp geçerli IP adresi olmalı")?;
            }
            cfg.server_public_ip = value.trim().to_string();
        }
        "trustedProxyCidrs" => {
            cfg.trusted_proxy_cidrs = value.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect();
            for cidr in &cfg.trusted_proxy_cidrs {
                cidr.parse::<ipnet::IpNet>().with_context(|| format!("Geçersiz trusted proxy CIDR: {cidr}"))?;
            }
        },
        "rateLimit.limit" => cfg.rate_limit.limit = value.parse().context("rateLimit.limit sayı olmalı")?,
        "rateLimit.window" => cfg.rate_limit.window = value.parse().context("rateLimit.window sayı olmalı")?,
        "priorityEvaluateIntervalDays" => cfg.priority_evaluate_interval_days = value.parse().context("priorityEvaluateIntervalDays sayı olmalı")?,
        "priorityTopCount" => cfg.priority_top_count = value.parse().context("priorityTopCount sayı olmalı")?,
        "highTrafficThresholdRequestsPerWindow" => cfg.high_traffic_threshold_requests_per_window = value.parse().context("highTrafficThresholdRequestsPerWindow sayı olmalı")?,
        _ => anyhow::bail!("Bilinmeyen ayar: {key}"),
    }

    save(&cfg)?;
    println!("{key} güncellendi.");
    Ok(())
}

pub fn import_legacy(dir: &PathBuf) -> Result<()> {
    for name in ["config.json", "vhosts.json", "blacklist.json", "priority_sites.json", "visit_stats.json", "dpsessions.json"] {
        let source = dir.join(name);
        let target = Path::new(name);

        if !source.exists() {
            continue;
        }

        let source_path = source.canonicalize()
            .with_context(|| format!("{} yolu çözümlenemedi", source.display()))?;
        if target.exists() {
            let target_path = target.canonicalize()
                .with_context(|| format!("{} yolu çözümlenemedi", target.display()))?;
            if source_path == target_path {
                println!("Atlandı (kaynak zaten hedef dosya): {name}");
                continue;
            }
        }

        let raw = fs::read_to_string(&source)
            .with_context(|| format!("{} okunamadı", source.display()))?;
        let _: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("{} geçersiz JSON", source.display()))?;

        if target.exists() {
            let backup = PathBuf::from(format!("{name}.pre-rust.bak"));
            if !backup.exists() {
                fs::copy(target, &backup)
                    .with_context(|| format!("{} için yedek oluşturulamadı", target.display()))?;
                println!("Yedeklendi: {}", backup.display());
            }
        }

        let tmp = PathBuf::from(format!("{name}.import.tmp"));
        fs::write(&tmp, raw)?;
        replace_file(&tmp, target)
            .with_context(|| format!("{name} içe aktarılamadı"))?;
        println!("İçe aktarıldı: {name}");
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


fn replace_file(temp: &Path, destination: &Path) -> Result<()> {
    match fs::rename(temp, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) if cfg!(windows) && destination.exists() => {
            fs::remove_file(destination).context("old config file could not be removed")?;
            fs::rename(temp, destination).context("new config file could not replace old one")?;
            Ok(())
        }
        Err(error) => Err(error).context("file rename failed"),
    }
}
