mod app;
mod cli;
mod config;
mod proxy;
mod state;
mod tui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Tui) {
        Command::Tui => tui::run().await,
        Command::Serve => proxy::serve().await,
        Command::Config { show } => config::print_config(show),
        Command::Vhosts => app::list_vhosts(),
        Command::Ban { ip } => app::set_ip_block(&ip, true),
        Command::Unban { ip } => app::set_ip_block(&ip, false),
        Command::WhitelistAdd { ip } => app::set_whitelist(&ip, true),
        Command::WhitelistRemove { ip } => app::set_whitelist(&ip, false),
        Command::Maintenance { enabled } => app::set_maintenance(enabled),
        Command::ImportLegacy { path } => config::import_legacy(&path),
    }
}
