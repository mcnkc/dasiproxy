use anyhow::Result;
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
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use std::{io::{self, Write}, time::Duration};
use tokio::task::JoinHandle;

const ITEMS: [&str; 14] = [
    "Overview",
    "Virtual Hosts",
    "Blacklist",
    "Whitelist",
    "Toggle Maintenance Mode",
    "Configuration",
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
    let mut proxy_task: Option<JoinHandle<Result<()>>> = None;
    let mut notice = String::from("Ready");
    loop {
        let cfg = crate::config::load()?;
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

            let menu: Vec<ListItem> = ITEMS.iter().enumerate().map(|(index, label)| {
                let prefix = if index == selected { "› " } else { "  " };
                let style = if index == selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(format!("{prefix}{label}")).style(style)
            }).collect();
            let proxy_status = if proxy_task.as_ref().is_some_and(|task| !task.is_finished()) { "running" } else { "stopped" };
            let menu_block = List::new(menu).block(
                Block::default().borders(Borders::ALL).title(format!(
                    " Proxy: {proxy_status}  |  Listen: {}:{}  |  Maintenance: {} ",
                    cfg.listen_address, cfg.listen_port,
                    if cfg.maintenance_mode { "ON" } else { "OFF" }
                ))
            );
            frame.render_widget(menu_block, areas[1]);
            let footer = Paragraph::new(format!("↑/↓ Navigate  Enter Select  q Quit  |  {notice}"))
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(footer, areas[2]);
        })?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => selected = (selected + 1).min(ITEMS.len() - 1),
                    KeyCode::Char('q') => break,
                    KeyCode::Enter => {
                        match selected {
                            0 => {
                                let state = crate::state::State::load()?;
                                notice = format!("{} vhosts • {} blocked • {} sessions",
                                    state.vhosts.len(), state.blacklist.len(), state.sessions.len());
                            }
                            1 => {
                                let state = crate::state::State::load()?;
                                notice = if state.vhosts.is_empty() { "No virtual hosts configured".into() }
                                    else { state.vhosts.iter().map(|(d,p)| format!("{d} → {p}")).collect::<Vec<_>>().join(" | ") };
                            }
                            2 => {
                                let state = crate::state::State::load()?;
                                notice = format!("Blacklist: {}", if state.blacklist.is_empty() { "empty".into() } else { state.blacklist.into_iter().collect::<Vec<_>>().join(", ") });
                            }
                            3 => {
                                let state = crate::state::State::load()?;
                                notice = format!("Whitelist: {}", if state.whitelist.is_empty() { "empty".into() } else { state.whitelist.into_iter().collect::<Vec<_>>().join(", ") });
                            }
                            4 => {
                                let mut cfg = crate::config::load()?;
                                cfg.maintenance_mode = !cfg.maintenance_mode;
                                crate::config::save(&cfg)?;
                                notice = format!("Maintenance mode {}", if cfg.maintenance_mode { "enabled" } else { "disabled" });
                            }
                            5 => notice = format!("Config: {}:{} • rate limit {}/{}ms",
                                cfg.listen_address, cfg.listen_port, cfg.rate_limit.limit, cfg.rate_limit.window),
                            6 => {
                                if proxy_task.as_ref().is_some_and(|task| !task.is_finished()) {
                                    if let Some(task) = proxy_task.take() { task.abort(); }
                                    notice = "Proxy stopped".into();
                                } else {
                                    proxy_task = Some(tokio::spawn(crate::proxy::serve()));
                                    notice = "Proxy starting…".into();
                                }
                            }
                            7 => {
                                if let Some(value) = prompt("Domain and local port (example.com:3000): ")? {
                                    if let Some((domain, port)) = value.rsplit_once(':') {
                                        if !domain.trim().is_empty() && port.parse::<u16>().is_ok() {
                                            let mut state = crate::state::State::load()?;
                                            state.vhosts.insert(domain.trim().to_ascii_lowercase(), port.to_string());
                                            state.save()?;
                                            notice = format!("Added {} → {}", domain.trim(), port);
                                        } else { notice = "Invalid domain or port".into(); }
                                    } else { notice = "Expected domain:port".into(); }
                                }
                            }
                            8 => {
                                if let Some(domain) = prompt("Domain to remove: ")? {
                                    let mut state = crate::state::State::load()?;
                                    if state.vhosts.remove(&domain.to_ascii_lowercase()).is_some() {
                                        state.save()?; notice = format!("Removed {domain}");
                                    } else { notice = "Domain not found".into(); }
                                }
                            }
                            9 | 10 | 11 | 12 => {
                                let prompt_text = match selected {
                                    9 => "IP to ban: ",
                                    10 => "IP to unban: ",
                                    11 => "IP to whitelist: ",
                                    _ => "IP to remove from whitelist: ",
                                };
                                if let Some(ip) = prompt(prompt_text)? {
                                    let mut state = crate::state::State::load()?;
                                    if selected == 11 || selected == 12 {
                                        state.whitelist(&ip, selected == 11)?;
                                        notice = format!("{} whitelist: {ip}", if selected == 11 { "Added to" } else { "Removed from" });
                                    } else {
                                        state.block(&ip, selected == 9)?;
                                        notice = format!("{} {ip}", if selected == 9 { "Banned" } else { "Unbanned" });
                                    }
                                }
                            }
                            13 => break,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(task) = proxy_task { task.abort(); }
    Ok(())
}

fn prompt(message: &str) -> Result<Option<String>> {
    disable_raw_mode()?;
    print!("\n{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    let result = io::stdin().read_line(&mut input);
    enable_raw_mode()?;
    result?;
    let input = input.trim().to_string();
    if input.is_empty() { Ok(None) } else { Ok(Some(input)) }
}
