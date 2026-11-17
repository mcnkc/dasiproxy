use anyhow::Result;
use chrono::Utc;
use std::{fs, path::Path};

use crate::state::State;

pub fn list_vhosts() -> Result<()> {
    let state = State::load()?;
    if state.vhosts.is_empty() {
        println!("Tanımlı vhost yok.");
    }
    for (domain, port) in state.vhosts {
        println!("{domain:<40} → {port}");
    }
    Ok(())
}

pub fn list_sessions() -> Result<()> {
    let state = State::load()?;
    println!("Tracked sessions: {}", state.sessions.len());
    for (fingerprint, sid) in state.sessions.iter().take(100) {
        let ip = fingerprint.split('|').next().unwrap_or("unknown");
        println!("{sid}  {ip}  {}", fingerprint.split('|').nth(1).unwrap_or("unknown"));
    }
    if state.sessions.len() > 100 {
        println!("... {} more sessions", state.sessions.len() - 100);
    }
    Ok(())
}

pub fn show_traffic() -> Result<()> {
    let state = State::load()?;
    let mut visits: Vec<_> = state.visit_stats.iter().collect();
    visits.sort_by(|a, b| b.1.cmp(a.1));
    println!("Hosts recorded: {}", state.visit_stats.len());
    println!("Priority hosts: {}", state.priority_sites.len());
    for (host, count) in visits.into_iter().take(20) {
        println!("{host:<40} {count:>10} requests");
    }
    if !state.priority_sites.is_empty() {
        println!("\nPriority allowlist:");
        for host in state.priority_sites {
            println!("  {host}");
        }
    }
    Ok(())
}

pub fn show_access_logs(lines: usize) -> Result<()> {
    let path = format!("logs/access-{}.log", Utc::now().format("%Y-%m-%d"));
    if !Path::new(&path).exists() {
        println!("No access log created for today.");
        return Ok(());
    }
    let content = fs::read_to_string(path)?;
    let entries: Vec<_> = content.lines().collect();
    let start = entries.len().saturating_sub(lines.min(10_000));
    for line in &entries[start..] {
        println!("{line}");
    }
    Ok(())
}

pub fn set_ip_block(ip: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    state.block(ip, enabled)?;
    println!("{}: {ip}", if enabled { "Engellendi" } else { "Engel kaldırıldı" });
    Ok(())
}

pub fn set_whitelist(ip: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    state.whitelist(ip, enabled)?;
    println!("{}: {ip}", if enabled { "Whitelist'e eklendi" } else { "Whitelist'ten kaldırıldı" });
    Ok(())
}

pub fn set_maintenance(enabled: bool) -> Result<()> {
    let mut cfg = crate::config::load()?;
    cfg.maintenance_mode = enabled;
    crate::config::save(&cfg)?;
    println!("Bakım modu: {}", if enabled { "aktif" } else { "pasif" });
    Ok(())
}


pub fn show_status() -> Result<()> {
    let state = State::load()?;
    let metrics = crate::metrics::snapshot();
    println!("dasiiProxy v3");
    println!("Uptime: {} seconds", metrics.uptime_seconds);
    println!("Total requests: {}", metrics.total_requests);
    println!("Completed requests: {}", metrics.completed_requests);
    println!("In-flight requests: {}", metrics.active_requests);
    println!("AI crawler blocks: {}", metrics.ai_bot_blocks);
    println!("Average response: {} ms", metrics.average_response_ms);
    println!("Virtual hosts: {}", state.vhosts.len());
    println!("Sessions: {}", state.sessions.len());
    println!("Blacklist: {} | Whitelist: {}", state.blacklist.len(), state.whitelist.len());
    Ok(())
}


pub fn show_runtime_logs(lines: usize) -> Result<()> {
    let directory = Path::new("logs");
    if !directory.exists() {
        println!("No runtime logs found.");
        return Ok(());
    }
    let mut files = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("runtime.log")))
        .collect::<Vec<_>>();
    files.sort();
    let Some(path) = files.pop() else {
        println!("No runtime logs found.");
        return Ok(());
    };
    let content = fs::read_to_string(path)?;
    let entries: Vec<_> = content.lines().collect();
    let start = entries.len().saturating_sub(lines.min(10_000));
    for line in &entries[start..] {
        println!("{line}");
    }
    Ok(())
}
