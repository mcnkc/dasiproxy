use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{body::Incoming, header, Request, Response, StatusCode};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
};
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Instant, collections::HashMap};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::{config, state::State};

type ProxyBody = BoxBody<Bytes, hyper::Error>;

fn full(value: impl Into<Bytes>) -> ProxyBody {
    Full::new(value.into())
        .map_err(|never: Infallible| match never {})
        .boxed()
}

pub async fn serve() -> Result<()> {
    let cfg = config::load()?;
    let state = Arc::new(tokio::sync::RwLock::new(State::load()?));
    let rate_state = Arc::new(tokio::sync::Mutex::new(HashMap::<String, (Instant, u64)>::new()));

    let addr: SocketAddr = format!("{}:{}", cfg.listen_address, cfg.listen_port)
        .parse()
        .context("Proxy listen address geçersiz")?;
    let listener = TcpListener::bind(addr)
        .await
        .context("Proxy portuna bind edilemedi (80 için sudo veya CAP_NET_BIND_SERVICE gerekebilir)")?;
    let connector = HttpConnector::new();
    let client: Client<_, Incoming> = Client::builder(TokioExecutor::new()).build(connector);

    info!("dasiiProxy v3 dinliyor: {addr}");

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
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ip = peer.ip().to_string();

    let snapshot = state.read().await.clone();

    if snapshot.blacklist.contains(&ip) && !snapshot.whitelist.contains(&ip) {
        return Ok(Response::builder().status(StatusCode::FORBIDDEN).body(full("403 Forbidden")).unwrap());
    }

    let cfg = match config::load() {
        Ok(config) => config,
        Err(err) => {
            error!(error = %err, "cannot load configuration");
            return Ok(Response::builder().status(500).body(full("500 Configuration Error")).unwrap());
        }
    };

    if cfg.maintenance_mode && !snapshot.whitelist.contains(&ip) {
        return Ok(Response::builder().status(StatusCode::SERVICE_UNAVAILABLE).body(full("503 Maintenance")).unwrap());
    }

    if !snapshot.whitelist.contains(&ip) && cfg.rate_limit.limit > 0 {
        let mut limits = rate_state.lock().await;
        let window = std::time::Duration::from_millis(cfg.rate_limit.window.max(1));
        let now = Instant::now();
        let entry = limits.entry(ip.clone()).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > cfg.rate_limit.limit {
            warn!(client_ip = %ip, "rate limit exceeded");
            return Ok(Response::builder().status(StatusCode::TOO_MANY_REQUESTS).body(full("429 Too Many Requests")).unwrap());
        }
        if limits.len() > 10_000 {
            limits.retain(|_, (start, _)| now.duration_since(*start) < window);
        }
    }

    let target = match snapshot.vhosts.get(&host) {
        Some(target) if target == "maintenance" => {
            return Ok(Response::builder().status(StatusCode::SERVICE_UNAVAILABLE).body(full("503 Maintenance")).unwrap());
        }
        Some(target) => target.clone(),
        None => return Ok(Response::builder().status(StatusCode::NOT_FOUND).body(full("404 Unknown Host")).unwrap()),
    };

    if target.parse::<u16>().is_err() {
        return Ok(Response::builder().status(StatusCode::BAD_GATEWAY).body(full("502 Invalid upstream port")).unwrap());
    }

    let path = req.uri().path_and_query().map(|value| value.as_str()).unwrap_or("/");
    let upstream_uri = match format!("http://127.0.0.1:{target}{path}").parse() {
        Ok(uri) => uri,
        Err(_) => return Ok(Response::builder().status(400).body(full("400 Bad Request")).unwrap()),
    };
    *req.uri_mut() = upstream_uri;

    if let Ok(value) = format!("127.0.0.1:{target}").parse() {
        req.headers_mut().insert(header::HOST, value);
    }
    if let Ok(value) = ip.parse() {
        req.headers_mut().insert("x-forwarded-for", value);
    }
    req.headers_mut().insert("x-forwarded-host", host.parse().unwrap_or_else(|_| header::HeaderValue::from_static("")));
    req.headers_mut().insert("x-forwarded-proto", header::HeaderValue::from_static("http"));

    match client.request(req).await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Ok(Response::from_parts(parts, body.map_err(|err| err).boxed()))
        }
        Err(err) => {
            error!(domain = %host, error = %err, "upstream request failed");
            Ok(Response::builder().status(StatusCode::BAD_GATEWAY).body(full("502 Bad Gateway")).unwrap())
        }
    }
}
