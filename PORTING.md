# v2 → Rust v3 porting status

This is the migration checklist for the Rust rewrite. The default branch remains the
known-good Node.js v2 version while this branch is developed.

| v2 capability | Rust v3 status | Notes |
|---|---|---|
| HTTP reverse proxy | ✅ Initial port | Hyper client/server, local port upstreams |
| WebSocket proxy | ✅ Initial port | HTTP/1.1 Upgrade tunnel; integration testing still required |
| Virtual hosts | ✅ Initial port | Reads/writes `vhosts.json`; TUI add/remove |
| Hot domain reload | ✅ Initial port | JSON state is reloaded once per second |
| Blacklist / whitelist | ✅ Initial port | IP validation and persisted lists |
| Rate limiting | ✅ Initial port | Per-IP rolling window |
| Automatic IP ban | ✅ Initial port | Triggered after 5× configured request limit |
| Priority traffic bypass | ✅ Initial port | Tracks per-host requests and periodically rebuilds `priority_sites.json` using the configured interval and top-host count |
| User-Agent validation | ✅ Initial port | Rejects missing/short User-Agent values |
| AI crawler blocking | ✅ Initial port | Configurable token-based filter |
| Maintenance mode | ✅ Initial port | Global and per-vhost maintenance response |
| dpSession IDs | ✅ Initial port | Fingerprint + UUID, persisted in `dpsessions.json` |
| dpSession ban API | ✅ Initial port | Optional authenticated query endpoint supports ban/unban by session UUID; integration tests still required |
| Daily access logs | ✅ Initial port | `logs/access-YYYY-MM-DD.log` |
| Static error pages | ✅ Initial port | Existing `staticPages/*.html` preserved |
| Runtime logs | ✅ Initial port | Daily rotating tracing output |
| Cloudflare DNS records | 🟡 Partial | TUI creates proxied A records for new vhosts; delete/update and full lifecycle handling pending |
| Discord alerts | ✅ Initial port | Upstream and WebSocket failures trigger a per-domain webhook alert with a five-minute cooldown |
| Public IP auto-detection | ✅ Initial port | Looks up the public IP during startup and stores it in config; network failure is non-fatal |
| Dashboard login/password | 🔁 Replaced | No network dashboard; local TUI is the control surface |
| Dashboard REST API | 🔁 Replaced | CLI commands and TUI actions instead |
| TLS termination | ⏳ Pending / deployment-specific | Current design expects Cloudflare or another TLS edge to forward HTTP |
| Linux / Windows installer | ✅ Initial port | Cargo-based installers; Rust toolchain required |

## Test gates before production

- Build and unit tests on Linux and Windows
- HTTP method, body streaming, headers, redirects, and error-page tests\n- Priority evaluation timing and ranking tests
- WebSocket handshake and long-lived bidirectional traffic test
- Rate-limit / auto-ban / allowlist integration tests
- Cloudflare DNS, Discord webhook, and dpSession API integration tests
- Load test and memory profiling
- Run in parallel with v2 before switching DNS or service ports
