mod app;
mod cli;
mod config;
mod integrations;
mod metrics;
mod proxy;
mod state;
mod tui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    std::fs::create_dir_all("logs")?;
    let file_appender = tracing_appender::rolling::daily("logs", "runtime.log");
    let (log_writer, _log_guard) = tracing_appender::non_blocking(file_appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(log_writer).init();

    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Tui) {
        Command::Tui => tui::run().await,
        Command::Serve => proxy::serve().await,
        Command::Config { show } => config::print_config(show),
        Command::ConfigSet { key, value } => config::set_value(&key, &value),
        Command::Status => app::show_status(),
        Command::Vhosts => app::list_vhosts(),
        Command::VhostAdd { domain, port } => app::add_vhost(&domain, port).await,
        Command::VhostRemove { domain } => app::remove_vhost(&domain).await,
        Command::Sessions { search, page, per_page } => app::list_sessions(search.as_deref(), page, per_page),
        Command::SessionBan { session_id } => app::set_session_block(&session_id, true),
        Command::SessionUnban { session_id } => app::set_session_block(&session_id, false),
        Command::Traffic => app::show_traffic(),
        Command::Logs { lines, system } => {
            if system { app::show_runtime_logs(lines) } else { app::show_access_logs(lines) }
        },
        Command::Ban { ip } => app::set_ip_block(&ip, true),
        Command::Unban { ip } => app::set_ip_block(&ip, false),
        Command::WhitelistAdd { ip } => app::set_whitelist(&ip, true),
        Command::WhitelistRemove { ip } => app::set_whitelist(&ip, false),
        Command::Maintenance { enabled } => app::set_maintenance(enabled),
        Command::ImportLegacy { path } => config::import_legacy(&path),
    }
}
