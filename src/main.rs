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
        Command::Vhosts => app::list_vhosts(),
        Command::Sessions => app::list_sessions(),
        Command::Traffic => app::show_traffic(),
        Command::Logs { lines } => app::show_access_logs(lines),
        Command::Ban { ip } => app::set_ip_block(&ip, true),
        Command::Unban { ip } => app::set_ip_block(&ip, false),
        Command::WhitelistAdd { ip } => app::set_whitelist(&ip, true),
        Command::WhitelistRemove { ip } => app::set_whitelist(&ip, false),
        Command::Maintenance { enabled } => app::set_maintenance(enabled),
        Command::ImportLegacy { path } => config::import_legacy(&path),
    }
}
