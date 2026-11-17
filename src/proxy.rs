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
use tokio::{fs::{self, OpenOptions}, io::AsyncWriteExt, net::TcpListener, time::{interval, Duration}};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{config, state::State};

type ProxyBody = BoxBody<Bytes, hyper::Error>;

fn full(value: impl Into<Bytes>) -> ProxyBody {
    Full::new(value.into()).map_err(|never: Infallible| match never {}).boxed()
}

pub async fn serve() -> Result<()> {
    let cfg = config::load()?;
    let state = Arc::new(tokio::sync::RwLock::new(State::load()?));
    let rate_state = Arc::new(tokio::sync::Mutex::new(HashMap::<String, (Instant, u64)>::new()));

    // Re-read JSON data periodically so TUI/CLI edits take effect without restarting.
    let state_refresh = state.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            match State::load() {
                Ok(fresh) => *state_refresh.write().await = fresh,
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
        let client = client.clone();

        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let state = state.clone();
                let rate_state = rate_state.clone();
                let client = client.clone();
                async move { handle(req, peer, state, rate_state, client).await }
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
    client: Client<HttpConnector, Incoming>,
) -> Result<Response<ProxyBody>, Infallible> {
    let raw_host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let host = normalize_host(raw_host);
    let ip = client_ip(&req, peer);

    let cfg = match config::load() {
        Ok(config) => config,
        Err(err) => {
            error!(error = %err, "cannot load configuration");
            return Ok(error_response(StatusCode::INTERNAL_SERVER_ERROR, 500, "Configuration error", true));
        }
    };

    let state_snapshot = state.read().await.clone();
    let whitelisted = state_snapshot.whitelist.contains(&ip);

    if state_snapshot.blacklist.contains(&ip) && !whitelisted {
        return Ok(error_response(StatusCode::FORBIDDEN, 403, "Forbidden", cfg.prefer_default_static_pages));
    }
    if cfg.maintenance_mode && !whitelisted {
        return Ok(error_response(StatusCode::SERVICE_UNAVAILABLE, 503, "Maintenance mode", cfg.prefer_default_static_pages));
    }

    if !whitelisted && cfg.rate_limit.limit > 0 {
        let mut limits = rate_state.lock().await;
        let window = Duration::from_millis(cfg.rate_limit.window.max(1));
        let now = Instant::now();
        let entry = limits.entry(ip.clone()).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > cfg.rate_limit.limit {
            warn!(client_ip = %ip, "rate limit exceeded");
            return Ok(error_response(StatusCode::TOO_MANY_REQUESTS, 429, "Too many requests", cfg.prefer_default_static_pages));
        }
        if limits.len() > 10_000 {
            limits.retain(|_, (start, _)| now.duration_since(*start) < window);
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

    match client.request(req).await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Ok(Response::from_parts(parts, body.map_err(|err| err).boxed()))
        }
        Err(err) => {
            error!(domain = %host, error = %err, "upstream request failed");
            Ok(error_response(StatusCode::BAD_GATEWAY, 502, "Bad Gateway", cfg.prefer_default_static_pages))
        }
    }
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
