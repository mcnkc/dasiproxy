# dasiiProxy v3 — Rust rewrite

A Rust-native reverse proxy with an interactive terminal dashboard (TUI).
The web dashboard and management HTTP API are replaced by a local terminal interface.

> The default branch still contains the original Node.js v2 application. This branch is
> an independent Rust v3 rewrite; it does not overwrite the existing release.

## Architecture

- Tokio async runtime and Hyper HTTP reverse proxy
- HTTP/1.1 WebSocket Upgrade tunnel
- Ratatui + Crossterm terminal dashboard
- Clap command-line administration
- Serde JSON configuration and legacy data files
- Tracing-based rotating runtime logs

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run -- tui
cargo run -- serve
cargo run -- status
cargo run -- vhosts
cargo run -- vhost-add example.com 3000
cargo run -- vhost-remove example.com
cargo run -- sessions --search 203.0.113.10 --page 1
cargo run -- session-ban SESSION_UUID
cargo run -- session-unban SESSION_UUID
cargo run -- traffic
cargo run -- logs --lines 80
cargo run -- logs --system --lines 80
cargo run -- config-set maintenanceMode true
cargo run -- config-set blockAIBots true
cargo run -- config-set trustedProxyCidrs "127.0.0.1/32,10.0.0.0/8"
cargo run -- ban 203.0.113.10
cargo run -- unban 203.0.113.10
cargo run -- whitelist-add 203.0.113.10
cargo run -- whitelist-remove 203.0.113.10
cargo run -- maintenance --enabled true
cargo run -- config --show
```

Running the proxy on port 80 generally requires elevated privileges or a Linux
capability such as `CAP_NET_BIND_SERVICE`. Set `listenPort` in `config.json`
to an unprivileged port while testing.

Forwarding headers such as `CF-Connecting-IP` and `X-Forwarded-For` are only
trusted when the direct TCP peer matches a CIDR in `trustedProxyCidrs`. Keep
this list empty when clients connect directly. When Cloudflare or another edge
proxy is in front, configure only that provider's current published proxy ranges;
do not trust arbitrary `X-Forwarded-For` values.

## Data compatibility

Import old v2 JSON files from an existing v2 directory with `dasiproxy import-legacy /path/to/old-dasiproxy`.
Existing target files are backed up as `*.pre-rust.bak` before import.

The Rust version reads/writes the existing file names where appropriate:
`config.json`, `vhosts.json`, `blacklist.json`, `priority_sites.json`,
`visit_stats.json`, and `dpsessions.json`. Legacy camelCase config keys are
supported.

## Rewrite status

The initial Rust version includes the TUI/CLI, HTTP and WebSocket proxying, vhost
routing, error pages, IP allow/deny lists, maintenance mode, request-method and
AI-crawler filters, per-IP rate limiting with automatic bans, session identifiers,
daily access logs, hot state reload, traffic counters and periodic priority-host
evaluation. TUI panels cover overview, vhosts, IP lists, sessions, traffic,
access logs and configuration.

Remaining parity work is tracked in [PORTING.md](PORTING.md). Do not replace a
production v2 instance until the remaining integrations are implemented and the
Rust proxy is tested against the actual deployment topology.
