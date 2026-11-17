use anyhow::{Context, Result};
use bytes::Bytes;
use chrono::Utc;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{body::Incoming, header, Request, Response, StatusCode};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
};
use std::{collections::HashMap, convert::Infallible, net::{IpAddr, SocketAddr}, sync::Arc, time::Instant};
use tokio::{
    fs::{self, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{interval, Duration},
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{config, state::{State, normalize_ip}};

type ProxyBody = BoxBody<Bytes, hyper::Error>;

fn full(value: impl Into<Bytes>) -> ProxyBody {
    Full::new(value.into()).map_err(|never: Infallible| match never {}).boxed()
}

pub async fn serve() -> Result<()> {
    let mut cfg = config::load()?;
    if cfg.server_public_ip.trim().is_empty() {
        match crate::integrations::discover_public_ip().await {
            Ok(ip) => {
                cfg.server_public_ip = ip;
                config::save(&cfg)?;
            }
            Err(err) => warn!(error = %err, "could not auto-detect public IP"),
        }
    }
    let state = Arc::new(tokio::sync::RwLock::new(State::load()?));
    let rate_state = Arc::new(tokio::sync::Mutex::new(HashMap::<String, (Instant, u64)>::new()));
    let traffic_window = Arc::new(tokio::sync::Mutex::new((Instant::now(), 0u64)));

    // Re-read JSON data periodically so TUI/CLI edits take effect without restarting.
    // Keep live request counters in memory and flush them independently from admin edits.
    let state_refresh = state.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        let mut last_visit_persist = Instant::now();
        let now = Utc::now().timestamp();
        let mut last_priority_evaluation = fs::read_to_string("priority_last_evaluated.txt")
            .await.ok().and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or_else(|| {
                let _ = std::fs::write("priority_last_evaluated.txt", now.to_string());
                now
            });

        loop {
            ticker.tick().await;
            match State::load() {
                Ok(mut fresh) => {
                    let mut current = state_refresh.write().await;
                    fresh.visit_stats = current.visit_stats.clone();
                    *current = fresh;

                    if last_visit_persist.elapsed() >= Duration::from_secs(15) {
                        if let Err(err) = current.save_visit_stats() {
                            warn!(error = %err, "traffic counters could not be persisted");
                        }
                        last_visit_persist = Instant::now();
                    }

                    let current_config = config::load().unwrap_or_default();
                    let now = Utc::now().timestamp();
                    let interval_seconds = current_config.priority_evaluate_interval_days.max(1) as i64 * 86_400;
                    if now - last_priority_evaluation >= interval_seconds {
                        match current.evaluate_priorities(current_config.priority_top_count) {
                            Ok(()) => {
                                last_priority_evaluation = now;
                                if let Err(err) = fs::write("priority_last_evaluated.txt", now.to_string()).await {
                                    warn!(error = %err, "priority evaluation timestamp could not be saved");
                                }
                                info!("traffic priority list evaluated");
                            }
                            Err(err) => warn!(error = %err, "priority evaluation failed"),
                        }
                    }
                }
                Err(err) => warn!(error = %err, "state refresh failed; keeping previous state"),
            }
        }
    });

    let addr: SocketAddr = format!("{}:{}", cfg.listen_address, cfg.listen_port)
        .parse()
        .context("Proxy listen address geçersiz")?;
    let listener = TcpListener::bind(addr)
        .await
        .context("Proxy portuna bind edilemedi (80 için sudo veya CAP_NET_BIND_SERVICE gerekebilir)")?;
    let connector = HttpConnector::new();
    let client: Client<_, Incoming> = Client::builder(TokioExecutor::new()).build(connector);

    info!("dasiiProxy v3 listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let state = state.clone();
        let rate_state = rate_state.clone();
        let traffic_window = traffic_window.clone();
        let client = client.clone();

        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let state = state.clone();
                let rate_state = rate_state.clone();
                let traffic_window = traffic_window.clone();
                let client = client.clone();
                async move { handle(req, peer, state, rate_state, traffic_window, client).await }
            });
            if let Err(err) = auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, service)
                .await
            {
                error!(%peer, error = %err, "connection failed");
            }
        });
    }
}

async fn handle(
    mut req: Request<Incoming>,
    peer: SocketAddr,
    state: Arc<tokio::sync::RwLock<State>>,
    rate_state: Arc<tokio::sync::Mutex<HashMap<String, (Instant, u64)>>>,
    traffic_window: Arc<tokio::sync::Mutex<(Instant, u64)>>,
    client: Client<HttpConnector, Incoming>,
) -> Result<Response<ProxyBody>, Infallible> {
    let raw_host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let host = normalize_host(&raw_host);
    let ip = client_ip(&req, peer);

    let cfg = match config::load() {
        Ok(config) => config,
        Err(err) => {
            error!(error = %err, "cannot load configuration");
            return Ok(error_response(StatusCode::INTERNAL_SERVER_ERROR, 500, "Configuration error", true));
        }
    };

    {
        let mut guard = state.write().await;
        *guard.visit_stats.entry(host.clone()).or_insert(0) += 1;
    }
    let state_snapshot = state.read().await.clone();

    if cfg.enable_dp_session_api
        && host.eq_ignore_ascii_case(&cfg.dp_session_api_domain)
        && req.uri().path().starts_with("/dpsession/api/")
    {
        return Ok(handle_dp_session_api(&req, &state, &cfg).await);
    }

    let whitelisted = state_snapshot.whitelist.contains(&ip);

    if state_snapshot.blacklist.contains(&ip) && !whitelisted {
        return Ok(error_response(StatusCode::FORBIDDEN, 403, "Forbidden", cfg.prefer_default_static_pages));
    }
    if cfg.maintenance_mode && !whitelisted {
        return Ok(error_response(StatusCode::SERVICE_UNAVAILABLE, 503, "Maintenance mode", cfg.prefer_default_static_pages));
    }

    let high_traffic = {
        let mut window = traffic_window.lock().await;
        let now = Instant::now();
        if now.duration_since(window.0) >= Duration::from_secs(60) {
            *window = (now, 0);
        }
        window.1 += 1;
        window.1 >= cfg.high_traffic_threshold_requests_per_window.max(1)
    };

    if !matches!(req.method().as_str(), "GET" | "POST" | "HEAD" | "OPTIONS") {
        return Ok(error_response(StatusCode::FORBIDDEN, 403, "Method not allowed", cfg.prefer_default_static_pages));
    }

    let user_agent = header_text(req.headers(), header::USER_AGENT);
    if user_agent.len() < 5 {
        return Ok(error_response(StatusCode::FORBIDDEN, 403, "User-Agent required", cfg.prefer_default_static_pages));
    }
    if cfg.block_ai_bots {
        let lower = user_agent.to_ascii_lowercase();
        const AI_BOT_TOKENS: [&str; 23] = [
            "claude", "searchbot", "gptbot", "chatgpt", "anthropic", "bytespider",
            "diffbot", "applebot", "amazonbot", "perplexity", "meta-externalfetcher",
            "meta-externalagent", "oai-searchbot", "bingbot", "googlebot",
            "google-cloudvertexbot", "googleother", "duckassistbot", "petalbot",
            "tiktokspider", "ccbot", "cohere-ai", "youbot",
        ];
        if AI_BOT_TOKENS.iter().any(|token| lower.contains(token)) {
            warn!(client_ip = %ip, user_agent = %user_agent, "AI crawler blocked");
            return Ok(error_response(StatusCode::FORBIDDEN, 403, "Automated crawler blocked", cfg.prefer_default_static_pages));
        }
    }

    if !whitelisted && cfg.rate_limit.limit > 0 {
        let now = Instant::now();
        let window = Duration::from_millis(cfg.rate_limit.window.max(1));
        let (limited, auto_ban) = {
            let mut limits = rate_state.lock().await;
            let entry = limits.entry(ip.clone()).or_insert((now, 0));
            if now.duration_since(entry.0) >= window {
                *entry = (now, 0);
            }
            entry.1 += 1;
            let count = entry.1;
            let limited = count > cfg.rate_limit.limit;
            let auto_ban = count > cfg.rate_limit.limit.saturating_mul(5);
            if limits.len() > 10_000 {
                limits.retain(|_, (start, _)| now.duration_since(*start) < window);
            }
            (limited, auto_ban)
        };

        if auto_ban {
            if let Err(err) = state.write().await.block(&ip, true) {
                warn!(client_ip = %ip, error = %err, "automatic IP ban could not be persisted");
            } else {
                warn!(client_ip = %ip, "automatic IP ban applied");
            }
            return Ok(error_response(StatusCode::FORBIDDEN, 403, "Automatically blocked", cfg.prefer_default_static_pages));
        }

        let priority_allowed = cfg.enable_priority_by_traffic
            && high_traffic
            && state_snapshot.priority_sites.contains(&host);
        if limited && !priority_allowed {
            warn!(client_ip = %ip, "rate limit exceeded");
            return Ok(error_response(StatusCode::TOO_MANY_REQUESTS, 429, "Too many requests", cfg.prefer_default_static_pages));
        }
    }

    let target = match state_snapshot.vhosts.get(&host) {
        Some(target) if target == "maintenance" => {
            return Ok(error_response(StatusCode::SERVICE_UNAVAILABLE, 503, "Maintenance", cfg.prefer_default_static_pages));
        }
        Some(target) => target.clone(),
        None => return Ok(error_response(StatusCode::NOT_FOUND, 404, "Unknown host", cfg.prefer_default_static_pages)),
    };
    if target.parse::<u16>().is_err() {
        return Ok(error_response(StatusCode::BAD_GATEWAY, 502, "Invalid upstream port", cfg.prefer_default_static_pages));
    }

    let session_id = {
        let user_agent = header_text(req.headers(), header::USER_AGENT);
        let language = req.headers().get("accept-language").and_then(|v| v.to_str().ok()).unwrap_or("unknown");
        let fingerprint = format!("{ip}|{}|{}", truncate(&user_agent, 256), truncate(language, 128));
        let mut guard = state.write().await;
        match guard.sessions.get(&fingerprint) {
            Some(existing) => existing.clone(),
            None => {
                if guard.sessions.len() > 50_000 {
                    guard.sessions.clear();
                }
                let id = Uuid::new_v4().to_string();
                guard.sessions.insert(fingerprint, id.clone());
                if let Err(err) = guard.save_sessions() {
                    warn!(error = %err, "could not persist session");
                }
                id
            }
        }
    };

    if cfg.enable_access_logs {
        if let Err(err) = write_access_log(&ip, &req, &host, &session_id).await {
            warn!(error = %err, "access log write failed");
        }
    }

    let path = req.uri().path_and_query().map(|value| value.as_str()).unwrap_or("/");
    let upstream_uri = match format!("http://127.0.0.1:{target}{path}").parse() {
        Ok(uri) => uri,
        Err(_) => return Ok(error_response(StatusCode::BAD_REQUEST, 400, "Bad request", cfg.prefer_default_static_pages)),
    };
    *req.uri_mut() = upstream_uri;

    if let Ok(value) = format!("127.0.0.1:{target}").parse() {
        req.headers_mut().insert(header::HOST, value);
    }
    if let Ok(value) = ip.parse() {
        req.headers_mut().insert("x-forwarded-for", value);
    }
    if let Ok(value) = raw_host.parse() {
        req.headers_mut().insert("x-forwarded-host", value);
    }
    req.headers_mut().insert("x-proxy-session-id", session_id.parse().unwrap_or_else(|_| header::HeaderValue::from_static("")));
    req.headers_mut().insert("x-forwarded-proto", header::HeaderValue::from_static("http"));

    if is_websocket_upgrade(&req) {
        return match websocket_proxy(req, &target).await {
            Ok(response) => Ok(response),
            Err(err) => {
                error!(domain = %host, error = %err, "websocket upstream failed");
                let alert_cfg = cfg.clone();
                let alert_domain = host.clone();
                let alert_error = err.to_string();
                tokio::spawn(async move {
                    if let Err(alert_err) = crate::integrations::notify_proxy_error(&alert_cfg, &alert_domain, &alert_error).await {
                        warn!(error = %alert_err, "Discord error alert failed");
                    }
                });
                Ok(error_response(StatusCode::BAD_GATEWAY, 502, "WebSocket Bad Gateway", cfg.prefer_default_static_pages))
            }
        };
    }

    match client.request(req).await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            for (name, value) in [
                (header::HeaderName::from_static("x-frame-options"), header::HeaderValue::from_static("DENY")),
                (header::HeaderName::from_static("x-content-type-options"), header::HeaderValue::from_static("nosniff")),
                (header::HeaderName::from_static("referrer-policy"), header::HeaderValue::from_static("no-referrer")),
                (header::HeaderName::from_static("permissions-policy"), header::HeaderValue::from_static("geolocation=(), camera=()")),
            ] {
                parts.headers.insert(name, value);
            }
            let status_code = parts.status.as_u16();
            if cfg.prefer_default_static_pages && matches!(status_code, 400 | 401 | 403 | 404 | 429 | 500 | 502 | 503 | 504 | 505) {
                return Ok(error_response(parts.status, status_code, "Upstream error", true));
            }
            Ok(Response::from_parts(parts, body.boxed()))
        }
        Err(err) => {
            error!(domain = %host, error = %err, "upstream request failed");
            let alert_cfg = cfg.clone();
            let alert_domain = host.clone();
            let alert_error = err.to_string();
            tokio::spawn(async move {
                if let Err(alert_err) = crate::integrations::notify_proxy_error(&alert_cfg, &alert_domain, &alert_error).await {
                    warn!(error = %alert_err, "Discord error alert failed");
                }
            });
            Ok(error_response(StatusCode::BAD_GATEWAY, 502, "Bad Gateway", cfg.prefer_default_static_pages))
        }
    }
}





async fn handle_dp_session_api(
    req: &Request<Incoming>,
    state: &Arc<tokio::sync::RwLock<State>>,
    cfg: &config::Config,
) -> Response<ProxyBody> {
    let query = req.uri().query().unwrap_or("");
    let params = url::form_urlencoded::parse(query.as_bytes());
    let mut auth = None;
    let mut ban = None;
    let mut unban = None;
    for (key, value) in params {
        match key.as_ref() {
            "auth" => auth = Some(value.into_owned()),
            "ban" => ban = Some(value.into_owned()),
            "unban" => unban = Some(value.into_owned()),
            _ => {}
        }
    }

    if auth.as_deref() != Some(cfg.dp_session_api_key.as_str()) {
        return api_text_response(StatusCode::FORBIDDEN, "Geçersiz Yetki Kodu (Auth Mismatch)".into());
    }

    if let Some(session_id) = ban {
        let mut guard = state.write().await;
        let matching_ips: Vec<String> = guard.sessions.iter()
            .filter_map(|(fingerprint, sid)| {
                if sid == &session_id {
                    Some(fingerprint.split('|').next().unwrap_or("").to_string())
                } else { None }
            })
            .collect();
        let mut banned = Vec::new();
        for raw_ip in matching_ips {
            if let Ok(ip) = normalize_ip(&raw_ip) {
                if guard.blacklist.insert(ip.clone()) { banned.push(ip); }
            }
        }
        if let Err(err) = guard.save_blacklist() {
            warn!(error = %err, "dpSession API could not persist blacklist");
            return api_text_response(StatusCode::INTERNAL_SERVER_ERROR, "Kara liste kaydedilemedi".into());
        }
        let detail = if banned.is_empty() { "Eşleşen IP bulunamadı".into() } else { banned.join(", ") };
        return api_text_response(StatusCode::OK, format!("Session [ {session_id} ] BANLANDI. Yasaklanan IP'ler: {detail}"));
    }

    if let Some(session_id) = unban {
        let mut guard = state.write().await;
        let matching_ips: Vec<String> = guard.sessions.iter()
            .filter_map(|(fingerprint, sid)| {
                if sid == &session_id {
                    Some(fingerprint.split('|').next().unwrap_or("").to_string())
                } else { None }
            })
            .collect();
        let mut unbanned = Vec::new();
        for raw_ip in matching_ips {
            if let Ok(ip) = normalize_ip(&raw_ip) {
                if guard.blacklist.remove(&ip) { unbanned.push(ip); }
            }
        }
        if let Err(err) = guard.save_blacklist() {
            warn!(error = %err, "dpSession API could not persist blacklist");
            return api_text_response(StatusCode::INTERNAL_SERVER_ERROR, "Kara liste kaydedilemedi".into());
        }
        let detail = if unbanned.is_empty() { "Eşleşen IP bulunamadı".into() } else { unbanned.join(", ") };
        return api_text_response(StatusCode::OK, format!("Session [ {session_id} ] BAN KALDIRILDI. İzin açılan IP'ler: {detail}"));
    }

    api_text_response(StatusCode::BAD_REQUEST, "Geçersiz İstek (Parametre Eksik)".into())
}

fn api_text_response(status: StatusCode, body: String) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(full(body))
        .unwrap()
}

fn is_websocket_upgrade(req: &Request<Incoming>) -> bool {
    let upgrade = req.headers().get(header::UPGRADE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let connection = req.headers().get(header::CONNECTION).and_then(|v| v.to_str().ok()).unwrap_or("");
    upgrade.eq_ignore_ascii_case("websocket")
        && connection.split(',').any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}

async fn websocket_proxy(mut req: Request<Incoming>, target: &str) -> Result<Response<ProxyBody>> {
    let on_upgrade = hyper::upgrade::on(&mut req);
    let port = target.parse::<u16>().context("invalid WebSocket upstream port")?;
    let mut upstream = TcpStream::connect(("127.0.0.1", port)).await
        .context("could not connect to WebSocket upstream")?;

    let path = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let mut head = format!("{} {} HTTP/1.1\r\n", req.method(), path);
    for (name, value) in req.headers() {
        let value = value.to_str().context("invalid request header encoding")?;
        head.push_str(name.as_str());
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    upstream.write_all(head.as_bytes()).await.context("could not forward WebSocket handshake")?;

    let mut response_bytes = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    let header_end = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let n = upstream.read(&mut chunk).await?;
            if n == 0 { anyhow::bail!("upstream closed before WebSocket handshake completed"); }
            response_bytes.extend_from_slice(&chunk[..n]);
            if response_bytes.len() > 65_536 { anyhow::bail!("upstream WebSocket headers exceed 64 KiB"); }
            if let Some(pos) = find_header_end(&response_bytes) { break Ok::<usize, anyhow::Error>(pos + 4); }
        }
    }).await.context("WebSocket handshake timed out")??;

    let header_text = std::str::from_utf8(&response_bytes[..header_end])
        .context("upstream returned non-UTF8 WebSocket headers")?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().context("missing WebSocket status line")?;
    let status_number = status_line.split_whitespace().nth(1)
        .context("invalid WebSocket status line")?.parse::<u16>()?;
    let status = StatusCode::from_u16(status_number)?;
    if status != StatusCode::SWITCHING_PROTOCOLS {
        anyhow::bail!("upstream rejected WebSocket upgrade with status {status}");
    }

    let mut builder = Response::builder().status(status);
    for line in lines {
        if line.is_empty() { continue; }
        if let Some((name, value)) = line.split_once(':') {
            let name = header::HeaderName::from_bytes(name.trim().as_bytes())?;
            let value = header::HeaderValue::from_bytes(value.trim().as_bytes())?;
            builder = builder.header(name, value);
        }
    }
    let response = builder.body(full(Bytes::new()))?;
    let leftover = response_bytes[header_end..].to_vec();

    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                let mut downstream = TokioIo::new(upgraded);
                if !leftover.is_empty() {
                    if downstream.write_all(&leftover).await.is_err() { return; }
                }
                if let Err(err) = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await {
                    warn!(error = %err, "WebSocket tunnel closed with an error");
                }
            }
            Err(err) => warn!(error = %err, "client WebSocket upgrade failed"),
        }
    });
    Ok(response)
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn client_ip(req: &Request<Incoming>, peer: SocketAddr) -> String {
    for name in ["cf-connecting-ip", "x-forwarded-for"] {
        if let Some(value) = req.headers().get(name).and_then(|v| v.to_str().ok()) {
            let first = value.split(',').next().unwrap_or("").trim();
            if let Ok(ip) = first.parse::<IpAddr>() { return ip.to_string(); }
        }
    }
    peer.ip().to_string()
}

fn normalize_host(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with('[') {
        return raw.split(']').next().unwrap_or(raw).trim_start_matches('[').to_ascii_lowercase();
    }
    raw.rsplit_once(':').map(|(host, _)| host).unwrap_or(raw).to_ascii_lowercase()
}

fn header_text(headers: &hyper::HeaderMap, name: header::HeaderName) -> String {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("unknown").to_string()
}

fn truncate(value: &str, max: usize) -> &str {
    let end = value.char_indices().map(|(i, _)| i).filter(|i| *i <= max).last().unwrap_or(0);
    if value.len() <= max { value } else { &value[..end] }
}

async fn write_access_log(ip: &str, req: &Request<Incoming>, host: &str, sid: &str) -> Result<()> {
    fs::create_dir_all("logs").await?;
    let now = Utc::now();
    let path = format!("logs/access-{}.log", now.format("%Y-%m-%d"));
    let url = req.uri().path_and_query().map(|v| v.as_str()).unwrap_or("/");
    let line = format!(
        "[{}] [SESSION: {}] IP: {} | METHOD: {} | URL: {}{}\n",
        now.format("%Y-%m-%d %H:%M:%S"), sid, ip, req.method(), host, url
    );
    let mut file = OpenOptions::new().create(true).append(true).open(path).await?;
    file.write_all(line.as_bytes()).await?;
    Ok(())
}

fn error_response(status: StatusCode, code: u16, fallback: &str, use_static: bool) -> Response<ProxyBody> {
    let page = if use_static {
        std::fs::read_to_string(format!("staticPages/{code}.html")).unwrap_or_else(|_| fallback.to_string())
    } else {
        fallback.to_string()
    };
    let content_type = if use_static { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" };
    Response::builder().status(status).header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(full(page)).unwrap()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_port_from_hostname() {
        assert_eq!(normalize_host("example.com:8080"), "example.com");
    }

    #[test]
    fn handles_bracketed_ipv6_host() {
        assert_eq!(normalize_host("[::1]:8080"), "::1");
    }

    #[test]
    fn truncates_only_at_utf8_boundaries() {
        let input = "a".repeat(254) + "ğ";
        assert_eq!(truncate(&input, 255).len(), 254);
    }

    #[test]
    fn finds_end_of_http_headers() {
        assert_eq!(find_header_end(b"HTTP/1.1 101 Switching Protocols\r\n\r\nframe"), Some(32));
    }
}
