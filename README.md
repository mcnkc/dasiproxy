# dasiiProxy v3 — Rust rewrite

A Rust-native reverse proxy with an interactive terminal dashboard (TUI).
The web dashboard and management HTTP API are replaced by a local terminal interface.

> The default branch still contains the original Node.js v2 application. This branch is
> an independent Rust v3 rewrite; it does not overwrite the existing release.

## Architecture

- Tokio async runtime
- Hyper HTTP reverse proxy
- Ratatui + Crossterm terminal dashboard
- Clap command-line administration
- Serde JSON configuration and legacy data files
- Tracing-based runtime logs

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run -- tui
cargo run -- serve
cargo run -- vhosts
cargo run -- ban 203.0.113.10
cargo run -- unban 203.0.113.10
cargo run -- maintenance --enabled true
cargo run -- config --show
```

Running the proxy on port 80 generally requires elevated privileges or a Linux
capability such as `CAP_NET_BIND_SERVICE`. Set `listenPort` in `config.json`
to an unprivileged port while testing.

## Data compatibility

The Rust version reads/writes the existing file names where appropriate:
`config.json`, `vhosts.json`, `blacklist.json`, `priority_sites.json`,
`visit_stats.json`, and `dpsessions.json`. Legacy camelCase config keys are
supported.

## Rewrite status

The initial Rust application includes the CLI, terminal menu, JSON state handling,
IP allow/deny lists, maintenance mode, per-IP rate limiting, session identifiers,
daily access logs, and an HTTP reverse-proxy foundation.

Full production parity is not yet complete. WebSocket tunnelling, TLS termination
(if needed outside Cloudflare), Cloudflare DNS automation, Discord alerts, advanced
traffic prioritisation, AI-bot filtering, and all legacy edge-case behaviours still
need to be ported and tested before replacing a production v2 instance.
