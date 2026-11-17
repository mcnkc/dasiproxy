use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "dasiproxy", version, about = "dasiiProxy v3 — Rust reverse proxy")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the interactive terminal dashboard
    Tui,
    /// Start the reverse proxy without opening the TUI
    Serve,
    /// Display or validate configuration
    Config { #[arg(long)] show: bool },
    /// List configured virtual hosts
    Vhosts,
    /// Add an IP address to the blacklist
    Ban { ip: String },
    /// Remove an IP address from the blacklist
    Unban { ip: String },
    /// Add an IP address to the whitelist
    WhitelistAdd { ip: String },
    /// Remove an IP address from the whitelist
    WhitelistRemove { ip: String },
    /// Enable or disable maintenance mode (example: maintenance --enabled true)
    Maintenance { #[arg(long, action = clap::ArgAction::Set)] enabled: bool },
    /// Import legacy dasiiProxy JSON files from a directory
    ImportLegacy { #[arg(default_value = ".")] path: PathBuf },
}
