use anyhow::Result;
use chrono::Utc;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::Duration,
};
use tokio::task::JoinHandle;

const ITEMS: [&str; 28] = [
    "Overview",
    "Virtual Hosts",
    "Blacklist",
    "Whitelist",
    "Sessions",
    "Session History",
    "Traffic & Priority",
    "Access Logs",
    "Runtime Logs",
    "Configuration",
    "Toggle Maintenance Mode",
    "Toggle AI Bot Blocking",
    "Toggle Access Logs",
    "Toggle Static Error Pages",
    "Toggle Priority Routing",
    "Toggle dpSession API",
    "Configure dpSession API",
    "Configure Discord Webhook",
    "Toggle Cloudflare DNS",
    "Configure Cloudflare DNS",
    "Start / Stop Proxy",
    "Add Virtual Host",
    "Remove Virtual Host",
    "Ban IP",
    "Unban IP",
    "Add IP to Whitelist",
    "Remove IP from Whitelist",
    "Quit",
];

pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let result = draw_loop(&mut terminal).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}

async fn draw_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut selected = 0usize;
    let mut list_state = ListState::default();
    list_state.select(Some(selected));
    let mut proxy_task: Option<JoinHandle<Result<()>>> = None;
    let mut notice = String::from("Ready");
    let mut detail = String::from("Choose a panel and press Enter.\nPress r to refresh the current panel.");

    loop {
        let cfg = crate::config::load()?;
        let proxy_running = proxy_task.as_ref().is_some_and(|task| !task.is_finished());
        terminal.draw(|frame| {
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(6), Constraint::Length(3)])
                .split(frame.area());

            let header = Paragraph::new(Line::from(vec![
                Span::styled(" dasii", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Proxy "),
                Span::styled("v3", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("  •  Rust-native reverse proxy"),
            ]))
            .block(Block::default().borders(Borders::ALL).title(" Control Center "));
            frame.render_widget(header, areas[0]);

            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(areas[1]);

            let menu: Vec<ListItem> = ITEMS.iter().map(|label| ListItem::new(*label)).collect();
            let menu_block = List::new(menu)
                .highlight_symbol("› ")
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .block(Block::default().borders(Borders::ALL).title(format!(
                    " Proxy: {} | {}:{} ",
                    if proxy_running { "running" } else { "stopped" },
                    cfg.listen_address,
                    cfg.listen_port
                )));
            frame.render_stateful_widget(menu_block, columns[0], &mut list_state);

            let panel = Paragraph::new(detail.as_str())
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title(ITEMS[selected]));
            frame.render_widget(panel, columns[1]);

            let footer = Paragraph::new(format!(
                "↑/↓ Navigate  Enter Select  r Refresh  q Quit  |  {}",
                notice
            ))
            .block(Block::default().borders(Borders::ALL));
            frame.render_widget(footer, areas[2]);
        })?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                        list_state.select(Some(selected));
                    }
                    KeyCode::Down => {
                        selected = (selected + 1).min(ITEMS.len() - 1);
                        list_state.select(Some(selected));
                    }
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => {
                        detail = panel_detail(selected)?;
                        notice = "Panel refreshed".into();
                    }
                    KeyCode::Enter => match selected {
                        0..=4 | 6..=9 => {
                            detail = panel_detail(selected)?;
                            notice = format!("{} refreshed", ITEMS[selected]);
                        }
                        5 => {
                            if let Some(sid) = prompt("Session UUID: ")? {
                                detail = session_history(&sid)?;
                                notice = format!("Session history loaded: {sid}");
                            }
                        }
                        10..=14 => {
                            let mut cfg = crate::config::load()?;
                            match selected {
                                10 => {
                                    cfg.maintenance_mode = !cfg.maintenance_mode;
                                    notice = format!("Maintenance mode {}", on_off(cfg.maintenance_mode));
                                }
                                11 => {
                                    cfg.block_ai_bots = !cfg.block_ai_bots;
                                    notice = format!("AI crawler blocking {}", on_off(cfg.block_ai_bots));
                                }
                                12 => {
                                    cfg.enable_access_logs = !cfg.enable_access_logs;
                                    notice = format!("Access logs {}", on_off(cfg.enable_access_logs));
                                }
                                13 => {
                                    cfg.prefer_default_static_pages = !cfg.prefer_default_static_pages;
                                    notice = format!("Static error pages {}", on_off(cfg.prefer_default_static_pages));
                                }
                                _ => {
                                    cfg.enable_priority_by_traffic = !cfg.enable_priority_by_traffic;
                                    notice = format!("Priority routing {}", on_off(cfg.enable_priority_by_traffic));
                                }
                            }
                            crate::config::save(&cfg)?;
                            detail = panel_detail(9)?;
                        }
                        15 => {
                            let mut cfg = crate::config::load()?;
                            if !cfg.enable_dp_session_api && cfg.dp_session_api_key.trim().is_empty() {
                                notice = "Configure a dpSession API key first".into();
                            } else {
                                cfg.enable_dp_session_api = !cfg.enable_dp_session_api;
                                notice = format!("dpSession API {}", on_off(cfg.enable_dp_session_api));
                                crate::config::save(&cfg)?;
                            }
                            detail = panel_detail(9)?;
                        }
                        16 => {
                            let domain = prompt("dpSession API domain: ")?;
                            let key = prompt("dpSession API secret: ")?;
                            if let (Some(domain), Some(key)) = (domain, key) {
                                let domain = domain.trim().to_ascii_lowercase();
                                if domain.is_empty() || key.trim().is_empty() {
                                    notice = "Domain and API secret must not be empty".into();
                                } else {
                                    let mut cfg = crate::config::load()?;
                                    cfg.dp_session_api_domain = domain.clone();
                                    cfg.dp_session_api_key = key.trim().to_string();
                                    cfg.enable_dp_session_api = true;
                                    crate::config::save(&cfg)?;
                                    notice = format!("dpSession API configured for {domain}");
                                }
                            }
                            detail = panel_detail(9)?;
                        }
                        17 => {
                            if let Some(webhook) = prompt_any("Discord webhook URL (empty clears it): ")? {
                                let mut cfg = crate::config::load()?;
                                cfg.discord_webhook = webhook.trim().to_string();
                                crate::config::save(&cfg)?;
                                notice = if cfg.discord_webhook.is_empty() {
                                    "Discord webhook cleared".into()
                                } else {
                                    "Discord webhook saved".into()
                                };
                                detail = panel_detail(9)?;
                            }
                        }
                        18 => {
                            let mut cfg = crate::config::load()?;
                            if !cfg.enable_cloudflare {
                                if cfg.cf_api_token.trim().is_empty() || cfg.cf_zone_id.trim().is_empty() {
                                    notice = "Configure Cloudflare API token and Zone ID first".into();
                                } else {
                                    cfg.enable_cloudflare = true;
                                    notice = "Cloudflare DNS enabled".into();
                                    crate::config::save(&cfg)?;
                                }
                            } else {
                                cfg.enable_cloudflare = false;
                                notice = "Cloudflare DNS disabled".into();
                                crate::config::save(&cfg)?;
                            }
                            detail = panel_detail(9)?;
                        }
                        19 => {
                            let token = prompt("Cloudflare API token: ")?;
                            let zone = prompt("Cloudflare Zone ID: ")?;
                            let public_ip = prompt_any("Public IP (empty = auto-detect): ")?;
                            if let (Some(token), Some(zone), Some(public_ip)) = (token, zone, public_ip) {
                                if token.trim().is_empty() || zone.trim().is_empty() {
                                    notice = "Cloudflare token and Zone ID are required".into();
                                } else {
                                    let mut cfg = crate::config::load()?;
                                    cfg.cf_api_token = token.trim().to_string();
                                    cfg.cf_zone_id = zone.trim().to_string();
                                    if !public_ip.trim().is_empty() {
                                        cfg.server_public_ip = public_ip.trim().to_string();
                                    }
                                    cfg.enable_cloudflare = true;
                                    crate::config::save(&cfg)?;
                                    notice = "Cloudflare settings saved and DNS enabled".into();
                                }
                            }
                            detail = panel_detail(9)?;
                        }
                        20 => {
                            if proxy_running {
                                if let Some(task) = proxy_task.take() {
                                    task.abort();
                                }
                                notice = "Proxy stopped".into();
                            } else {
                                proxy_task = Some(tokio::spawn(crate::proxy::serve()));
                                notice = "Proxy starting…".into();
                            }
                        }
                        21 => {
                            if let Some(value) = prompt("Domain and local port (example.com:3000): ")? {
                                if let Some((domain, port)) = value.rsplit_once(':') {
                                    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
                                    let valid_domain = !domain.is_empty()
                                        && domain.len() <= 253
                                        && domain.split('.').all(|part| {
                                            !part.is_empty()
                                                && part.len() <= 63
                                                && part.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                                                && !part.starts_with('-')
                                                && !part.ends_with('-')
                                        });
                                    if valid_domain && port.parse::<u16>().is_ok() && port != "0" {
                                        let mut state = crate::state::State::load()?;
                                        state.vhosts.insert(domain.clone(), port.to_string());
                                        state.save()?;
                                        let cfg = crate::config::load()?;
                                        match crate::integrations::create_cloudflare_record(&cfg, &domain).await {
                                            Ok(true) => notice = format!("Added {domain} → {port}; Cloudflare DNS created"),
                                            Ok(false) => notice = format!("Added {domain} → {port}"),
                                            Err(err) => notice = format!("Vhost added; Cloudflare error: {err}"),
                                        }
                                        detail = panel_detail(1)?;
                                    } else {
                                        notice = "Invalid domain or port".into();
                                    }
                                } else {
                                    notice = "Expected domain:port".into();
                                }
                            }
                        }
                        22 => {
                            if let Some(domain) = prompt("Domain to remove: ")? {
                                let domain = domain.trim().to_ascii_lowercase();
                                let mut state = crate::state::State::load()?;
                                if state.vhosts.remove(&domain).is_some() {
                                    state.save()?;
                                    let cfg = crate::config::load()?;
                                    match crate::integrations::delete_cloudflare_record(&cfg, &domain).await {
                                        Ok(true) => notice = format!("Removed {domain}; managed DNS record deleted"),
                                        Ok(false) => notice = format!("Removed {domain}; no managed DNS record found"),
                                        Err(err) => notice = format!("Vhost removed; Cloudflare delete error: {err}"),
                                    }
                                    detail = panel_detail(1)?;
                                } else {
                                    notice = "Domain not found".into();
                                }
                            }
                        }
                        23..=26 => {
                            let prompt_text = match selected {
                                23 => "IP to ban: ",
                                24 => "IP to unban: ",
                                25 => "IP to whitelist: ",
                                _ => "IP to remove from whitelist: ",
                            };
                            if let Some(ip) = prompt(prompt_text)? {
                                let mut state = crate::state::State::load()?;
                                if selected == 25 || selected == 26 {
                                    state.whitelist(&ip, selected == 25)?;
                                    notice = format!("{} whitelist: {ip}", if selected == 25 { "Added to" } else { "Removed from" });
                                    detail = panel_detail(3)?;
                                } else {
                                    state.block(&ip, selected == 23)?;
                                    notice = format!("{} {ip}", if selected == 23 { "Banned" } else { "Unbanned" });
                                    detail = panel_detail(2)?;
                                }
                            }
                        }
                        27 => break,
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
    if let Some(task) = proxy_task {
        task.abort();
    }
    Ok(())
}

fn panel_detail(selected: usize) -> Result<String> {
    let cfg = crate::config::load()?;
    let state = crate::state::State::load()?;
    let metrics = crate::metrics::snapshot();
    let detail = match selected {
        0 => format!(
            "dasiiProxy v3\n\nVirtual hosts: {}\nBlacklist entries: {}\nWhitelist entries: {}\nTracked sessions: {}\nTracked hosts: {}\nPriority hosts: {}\n\nListener: {}:{}\nMaintenance: {}\nAI bot blocking: {}\nAccess logs: {}\nStatic error pages: {}\nPriority routing: {}\ndpSession API: {}\nCloudflare DNS: {}",
            state.vhosts.len(), state.blacklist.len(), state.whitelist.len(),
            state.sessions.len(), state.visit_stats.len(), state.priority_sites.len(),
            cfg.listen_address, cfg.listen_port,
            on_off(cfg.maintenance_mode), on_off(cfg.block_ai_bots), on_off(cfg.enable_access_logs),
            on_off(cfg.prefer_default_static_pages), on_off(cfg.enable_priority_by_traffic),
            on_off(cfg.enable_dp_session_api), on_off(cfg.enable_cloudflare)
        ),
        1 => {
            let mut items: Vec<_> = state.vhosts.iter().collect();
            items.sort_by_key(|(domain, _)| *domain);
            if items.is_empty() {
                "No virtual hosts configured.".into()
            } else {
                items.into_iter().map(|(domain, port)| format!("{domain}  →  127.0.0.1:{port}")).collect::<Vec<_>>().join("\n")
            }
        }
        2 => show_set("Blacklisted IPs", state.blacklist.into_iter().collect()),
        3 => show_set("Whitelisted IPs", state.whitelist.into_iter().collect()),
        4 => {
            let mut sessions: Vec<_> = state.sessions.iter().collect();
            sessions.sort_by_key(|(_, sid)| *sid);
            let mut lines = vec![format!("Tracked sessions: {}", sessions.len()), String::new()];
            for (fingerprint, sid) in sessions.into_iter().take(100) {
                let ip = fingerprint.split('|').next().unwrap_or("unknown");
                let ua = fingerprint.split('|').nth(1).unwrap_or("unknown");
                lines.push(format!("{sid}\n  IP: {ip}\n  UA: {ua}"));
            }
            if lines.len() == 2 {
                lines.push("No sessions yet.".into());
            }
            if state.sessions.len() > 100 {
                lines.push(format!("… {} more", state.sessions.len() - 100));
            }
            lines.join("\n")
        }
        6 => {
            let mut visits: Vec<_> = state.visit_stats.iter().collect();
            visits.sort_by(|a, b| b.1.cmp(a.1));
            let mut lines = vec![
                format!("Hosts with traffic: {}", state.visit_stats.len()),
                format!("Priority hosts: {}", state.priority_sites.len()),
                String::new(),
                "Top request counters:".into(),
            ];
            for (host, count) in visits.into_iter().take(20) {
                lines.push(format!("{host:<32} {count:>10}"));
            }
            lines.push(String::new());
            lines.push("Priority allowlist:".into());
            let mut priority: Vec<_> = state.priority_sites.into_iter().collect();
            priority.sort();
            lines.extend(priority.into_iter().map(|host| format!("• {host}")));
            lines.join("\n")
        }
        7 => read_access_logs()?,
        8 => read_runtime_logs()?,
        9 => redacted_config(&cfg)?,
        _ => "Select a panel.".into(),
    };
    Ok(detail)
}

fn session_history(sid: &str) -> Result<String> {
    let path = format!("logs/access-{}.log", Utc::now().format("%Y-%m-%d"));
    if !Path::new(&path).exists() {
        return Ok("No access log has been written today.".into());
    }
    let content = fs::read_to_string(path)?;
    let lines: Vec<_> = content.lines()
        .filter(|line| line.contains(&format!("[SESSION: {sid}]")))
        .collect();
    if lines.is_empty() {
        Ok("No access history found for this session today.".into())
    } else {
        Ok(lines.into_iter().rev().take(50).rev().collect::<Vec<_>>().join("\n"))
    }
}

fn read_access_logs() -> Result<String> {
    let path = format!("logs/access-{}.log", Utc::now().format("%Y-%m-%d"));
    if !Path::new(&path).exists() {
        return Ok("No access log has been written today.".into());
    }
    let raw = fs::read_to_string(path)?;
    let lines: Vec<_> = raw.lines().collect();
    Ok(lines.iter().rev().take(40).rev().copied().collect::<Vec<_>>().join("\n"))
}

fn read_runtime_logs() -> Result<String> {
    if !Path::new("logs").exists() {
        return Ok("No runtime logs found.".into());
    }
    let mut files = fs::read_dir("logs")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("runtime.log")))
        .collect::<Vec<_>>();
    files.sort();
    let Some(path) = files.pop() else {
        return Ok("No runtime logs found.".into());
    };
    let raw = fs::read_to_string(path)?;
    let lines: Vec<_> = raw.lines().collect();
    Ok(lines.iter().rev().take(40).rev().copied().collect::<Vec<_>>().join("\n"))
}

fn redacted_config(cfg: &crate::config::Config) -> Result<String> {
    let mut value = serde_json::to_value(cfg)?;
    for key in ["dashboardPassword", "dpSessionApiKey", "discordWebhook", "cfApiToken"] {
        if value.get(key).and_then(|item| item.as_str()).is_some_and(|secret| !secret.is_empty()) {
            value[key] = serde_json::Value::String("********".into());
        }
    }
    Ok(serde_json::to_string_pretty(&value)?)
}

fn show_set(title: &str, mut values: Vec<String>) -> String {
    values.sort();
    if values.is_empty() {
        format!("{title}\n\nEmpty.")
    } else {
        format!("{title}\n\n{}", values.join("\n"))
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

fn prompt_any(message: &str) -> Result<Option<String>> {
    disable_raw_mode()?;
    print!("\\n{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    let result = io::stdin().read_line(&mut input);
    enable_raw_mode()?;
    result?;
    Ok(Some(input.trim().to_string()))
}

fn prompt(message: &str) -> Result<Option<String>> {
    let value = prompt_any(message)?;
    Ok(value.filter(|input| !input.is_empty()))
}
