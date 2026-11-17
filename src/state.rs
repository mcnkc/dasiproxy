use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::{HashMap, HashSet}, fs, net::IpAddr, path::Path};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub vhosts: HashMap<String, String>,
    pub blacklist: HashSet<String>,
    pub whitelist: HashSet<String>,
    pub visit_stats: HashMap<String, u64>,
    pub priority_sites: HashSet<String>,
    pub sessions: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct HotReloadState {
    pub vhosts: HashMap<String, String>,
    pub blacklist: HashSet<String>,
    pub priority_sites: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct RequestSnapshot {
    pub whitelisted: bool,
    pub blacklisted: bool,
    pub priority_site: bool,
    pub target: Option<String>,
}

impl State {
    pub fn load() -> Result<Self> {
        let mut state = Self::default();
        state.vhosts = read_json("vhosts.json")?;
        state.blacklist = read_json("blacklist.json")?;
        state.whitelist = crate::config::load()?.ip_whitelist.into_iter().collect();
        state.visit_stats = read_json("visit_stats.json")?;
        state.priority_sites = read_json("priority_sites.json")?;
        state.sessions = read_json("dpsessions.json")?;
        Ok(state)
    }

    pub fn load_hot_reload() -> Result<HotReloadState> {
        Ok(HotReloadState {
            vhosts: read_json("vhosts.json")?,
            blacklist: read_json("blacklist.json")?,
            priority_sites: read_json("priority_sites.json")?,
        })
    }

    pub fn request_snapshot(&self, ip: &str, host: &str) -> RequestSnapshot {
        RequestSnapshot {
            whitelisted: self.whitelist.contains(ip),
            blacklisted: self.blacklist.contains(ip),
            priority_site: self.priority_sites.contains(host),
            target: self.vhosts.get(host).cloned(),
        }
    }

    pub fn save(&self) -> Result<()> {
        write_json("vhosts.json", &self.vhosts)?;
        write_json("blacklist.json", &self.blacklist)?;
        write_json("visit_stats.json", &self.visit_stats)?;
        write_json("priority_sites.json", &self.priority_sites)?;
        // Session history is persisted exclusively by the background batch writer.
        Ok(())
    }

    pub fn save_sessions(&self) -> Result<()> {
        write_json("dpsessions.json", &self.sessions)
    }

    pub fn save_blacklist(&self) -> Result<()> {
        write_json("blacklist.json", &self.blacklist)
    }

    pub fn save_visit_stats(&self) -> Result<()> {
        write_json("visit_stats.json", &self.visit_stats)
    }

    pub fn evaluate_priorities(&mut self, top_count: usize) -> Result<()> {
        let mut ranked: Vec<_> = self.visit_stats.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1));
        self.priority_sites = ranked.into_iter()
            .take(top_count.max(1))
            .map(|(host, _)| host.clone())
            .collect();
        self.visit_stats.clear();
        write_json("priority_sites.json", &self.priority_sites)?;
        write_json("visit_stats.json", &self.visit_stats)
    }

    pub fn block(&mut self, raw: &str, enabled: bool) -> Result<()> {
        let ip = normalize_ip(raw)?;
        if enabled { self.blacklist.insert(ip); } else { self.blacklist.remove(&ip); }
        write_json("blacklist.json", &self.blacklist)
    }

    pub fn whitelist(&mut self, raw: &str, enabled: bool) -> Result<()> {
        let ip = normalize_ip(raw)?;
        if enabled { self.whitelist.insert(ip); } else { self.whitelist.remove(&ip); }
        let mut cfg = crate::config::load()?;
        cfg.ip_whitelist = self.whitelist.iter().cloned().collect();
        crate::config::save(&cfg)
    }

    pub fn set_session_ban(&mut self, session_id: &str, enabled: bool) -> Result<Vec<String>> {
        let matched_ips: HashSet<String> = self.sessions.iter()
            .filter_map(|(fingerprint, sid)| {
                if sid == session_id {
                    fingerprint.split('|').next().and_then(|raw| normalize_ip(raw).ok())
                } else {
                    None
                }
            })
            .collect();

        let mut changed = Vec::new();
        for ip in matched_ips {
            let updated = if enabled {
                self.blacklist.insert(ip.clone())
            } else {
                self.blacklist.remove(&ip)
            };
            if updated { changed.push(ip); }
        }
        changed.sort();
        self.save_blacklist()?;
        Ok(changed)
    }
}

pub fn normalize_ip(raw: &str) -> Result<String> {
    let first = raw.trim().split(',').next().unwrap_or("").trim();
    let ip: IpAddr = first.parse().context("Geçersiz IP adresi")?;
    Ok(ip.to_string())
}

fn read_json<T: DeserializeOwned + Default>(name: &str) -> Result<T> {
    if !Path::new(name).exists() { return Ok(T::default()); }
    let raw = fs::read_to_string(name).with_context(|| format!("{name} okunamadı"))?;
    serde_json::from_str(&raw).with_context(|| format!("{name} geçersiz JSON"))
}

fn write_json<T: Serialize>(name: &str, value: &T) -> Result<()> {
    let tmp = format!("{name}.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    match fs::rename(&tmp, name) {
        Ok(()) => Ok(()),
        Err(_) if cfg!(windows) && Path::new(name).exists() => {
            fs::remove_file(name).with_context(|| format!("{name} eski dosyası silinemedi"))?;
            fs::rename(&tmp, name).with_context(|| format!("{name} güncellenemedi"))?;
            Ok(())
        }
        Err(error) => Err(error).with_context(|| format!("{name} güncellenemedi")),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_ipv4_address() {
        assert_eq!(normalize_ip("203.0.113.7").unwrap(), "203.0.113.7");
    }

    #[test]
    fn normalizes_ipv6_address() {
        assert_eq!(normalize_ip("2001:db8::1").unwrap(), "2001:db8::1");
    }

    #[test]
    fn rejects_invalid_ip() {
        assert!(normalize_ip("not-an-ip").is_err());
    }

    #[test]
    fn request_snapshot_reads_only_the_requested_keys() {
        let mut state = State::default();
        state.vhosts.insert("example.com".into(), "3000".into());
        state.whitelist.insert("203.0.113.7".into());
        state.blacklist.insert("203.0.113.8".into());
        state.priority_sites.insert("example.com".into());

        let allowed = state.request_snapshot("203.0.113.7", "example.com");
        assert!(allowed.whitelisted);
        assert!(!allowed.blacklisted);
        assert!(allowed.priority_site);
        assert_eq!(allowed.target.as_deref(), Some("3000"));

        let blocked = state.request_snapshot("203.0.113.8", "missing.example");
        assert!(!blocked.whitelisted);
        assert!(blocked.blacklisted);
        assert!(!blocked.priority_site);
        assert_eq!(blocked.target, None);
    }

}
