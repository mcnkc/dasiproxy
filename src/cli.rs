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
    /// Update a config key (example: config-set maintenanceMode true)
    ConfigSet { key: String, value: String },
    /// List configured virtual hosts
    Vhosts,
    /// Add a virtual host and optionally create its managed Cloudflare DNS record
    VhostAdd { domain: String, port: u16 },
    /// Remove a virtual host and its managed Cloudflare DNS record
    VhostRemove { domain: String },
    /// Show runtime status and counters
    Status,
    /// List tracked dpSessions with search and pagination
    Sessions {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, default_value_t = 1)]
        page: usize,
        #[arg(long, default_value_t = 50)]
        per_page: usize,
    },
    /// Ban all IPs associated with a tracked session UUID
    SessionBan { session_id: String },
    /// Unban all IPs associated with a tracked session UUID
    SessionUnban { session_id: String },
    /// Display traffic counters and priority hosts
    Traffic,
    /// Show today's access log entries
    Logs {
        #[arg(long, default_value_t = 80)]
        lines: usize,
        /// Read rotating runtime/system logs instead of access logs
        #[arg(long)]
        system: bool,
    },
    /// Add an IP address to the blacklist
    Ban { ip: String },
    /// Remove an IP address from the blacklist
    Unban { ip: String },
    /// Add an IP address to the whitelist
    WhitelistAdd { ip: String },
    /// Remove an IP address from the whitelist
    WhitelistRemove { ip: String },
    /// Enable or disable maintenance mode
    Maintenance { #[arg(long, action = clap::ArgAction::Set)] enabled: bool },
    /// Import legacy dasiiProxy JSON files from a directory
    ImportLegacy { #[arg(default_value = ".")] path: PathBuf },
}
