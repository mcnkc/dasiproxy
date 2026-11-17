use anyhow::{Context, Result};
use chrono::Utc;
use std::{fs, path::Path};

use crate::state::State;

pub fn list_vhosts() -> Result<()> {
    let state = State::load()?;
    if state.vhosts.is_empty() {
        println!("Tanımlı vhost yok.");
    }
    let mut entries: Vec<_> = state.vhosts.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (domain, port) in entries {
        println!("{domain:<40} → 127.0.0.1:{port}");
    }
    Ok(())
}

pub fn list_sessions(search: Option<&str>, page: usize, per_page: usize) -> Result<()> {
    let state = State::load()?;
    let query = search.unwrap_or("").to_ascii_lowercase();
    let mut entries: Vec<_> = state.sessions.iter()
        .filter(|(fingerprint, sid)| {
            query.is_empty()
                || sid.to_ascii_lowercase().contains(&query)
                || fingerprint.to_ascii_lowercase().contains(&query)
        })
        .collect();
    entries.sort_by(|a, b| a.1.cmp(b.1));

    let page_size = per_page.clamp(1, 1000);
    let total = entries.len();
    let page_count = total.div_ceil(page_size).max(1);
    let current_page = page.max(1).min(page_count);
    let start = (current_page - 1) * page_size;

    println!("Sessions: {total} | Page: {current_page}/{page_count}");
    for (fingerprint, sid) in entries.into_iter().skip(start).take(page_size) {
        let ip = fingerprint.split('|').next().unwrap_or("unknown");
        let user_agent = fingerprint.split('|').nth(1).unwrap_or("unknown");
        let status = if state.blacklist.contains(ip) { "BANNED" } else { "ACTIVE" };
        println!("{sid}  {ip:<40} [{status}]  {user_agent}");
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
        let mut priority: Vec<_> = state.priority_sites.into_iter().collect();
        priority.sort();
        for host in priority {
            println!("  {host}");
        }
    }
    Ok(())
}

pub fn show_access_logs(lines: usize) -> Result<()> {
    let path = format!("logs/access-{}.log", Utc::now().format("%Y-%m-%d"));
    print_last_lines(&path, lines, "No access log created for today.")
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
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("runtime.log"))
        })
        .collect::<Vec<_>>();
    files.sort();
    let Some(path) = files.pop() else {
        println!("No runtime logs found.");
        return Ok(());
    };
    let content = fs::read_to_string(path)?;
    print_lines(content.lines().collect(), lines);
    Ok(())
}

pub fn set_ip_block(ip: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    state.block(ip, enabled)?;
    println!("{}: {ip}", if enabled { "Engellendi" } else { "Engel kaldırıldı" });
    Ok(())
}

pub fn set_session_block(session_id: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    let changed = state.set_session_ban(session_id, enabled)?;
    if changed.is_empty() {
        println!("Session için eşleşen IP bulunamadı: {session_id}");
    } else {
        println!("{} session {session_id}: {}", if enabled { "Banned" } else { "Unbanned" }, changed.join(", "));
    }
    Ok(())
}

pub fn set_whitelist(ip: &str, enabled: bool) -> Result<()> {
    let mut state = State::load()?;
    state.whitelist(ip, enabled)?;
    println!("{}: {ip}", if enabled { "Whitelist'e eklendi" } else { "Whitelist'ten kaldırıldı" });
    Ok(())
}

pub async fn add_vhost(domain: &str, port: u16) -> Result<()> {
    let domain = normalize_domain(domain)?;
    if port == 0 {
        anyhow::bail!("Vhost portu 1-65535 arası olmalı");
    }
    let mut state = State::load()?;
    if state.vhosts.contains_key(&domain) {
        anyhow::bail!("Vhost zaten tanımlı: {domain}");
    }
    state.vhosts.insert(domain.clone(), port.to_string());
    state.save()?;

    let cfg = crate::config::load()?;
    match crate::integrations::create_cloudflare_record(&cfg, &domain).await {
        Ok(true) => println!("Vhost eklendi: {domain} → 127.0.0.1:{port}; Cloudflare DNS created"),
        Ok(false) => println!("Vhost eklendi: {domain} → 127.0.0.1:{port}"),
        Err(error) => {
            println!("Vhost eklendi fakat Cloudflare DNS başarısız: {error}");
            return Err(error);
        }
    }
    Ok(())
}

pub async fn remove_vhost(domain: &str) -> Result<()> {
    let domain = normalize_domain(domain)?;
    let mut state = State::load()?;
    if state.vhosts.remove(&domain).is_none() {
        anyhow::bail!("Vhost bulunamadı: {domain}");
    }
    state.save()?;

    let cfg = crate::config::load()?;
    match crate::integrations::delete_cloudflare_record(&cfg, &domain).await {
        Ok(true) => println!("Vhost silindi: {domain}; managed Cloudflare DNS record deleted"),
        Ok(false) => println!("Vhost silindi: {domain}; managed DNS record not found"),
        Err(error) => {
            println!("Vhost silindi fakat Cloudflare DNS silinemedi: {error}");
            return Err(error);
        }
    }
    Ok(())
}

fn normalize_domain(raw: &str) -> Result<String> {
    let domain = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let valid = !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                && !part.starts_with('-')
                && !part.ends_with('-')
        });
    if !valid {
        anyhow::bail!("Geçersiz domain: {raw}");
    }
    Ok(domain)
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

fn print_last_lines(path: &str, lines: usize, empty_message: &str) -> Result<()> {
    if !Path::new(path).exists() {
        println!("{empty_message}");
        return Ok(());
    }
    let content = fs::read_to_string(path).with_context(|| format!("{path} okunamadı"))?;
    print_lines(content.lines().collect(), lines);
    Ok(())
}

fn print_lines(entries: Vec<&str>, lines: usize) {
    let start = entries.len().saturating_sub(lines.min(10_000));
    for line in &entries[start..] {
        println!("{line}");
    }
}
