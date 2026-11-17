use anyhow::{Context, Result};
use serde_json::json;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::{config, config::Config};

static ALERT_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

pub async fn discover_public_ip() -> Result<String> {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get("https://api.ipify.org")
        .send()
        .await
        .context("public IP lookup request failed")?
        .error_for_status()
        .context("public IP lookup returned an error")?;
    let value = response.text().await?.trim().to_string();
    let parsed: IpAddr = value.parse().context("public IP service returned an invalid address")?;
    Ok(parsed.to_string())
}

/// Creates a proxied Cloudflare A record for a newly-added virtual host.
/// Returns false when Cloudflare integration is disabled.
pub async fn create_cloudflare_record(cfg: &Config, domain: &str) -> Result<bool> {
    if !cfg.enable_cloudflare {
        return Ok(false);
    }
    if cfg.cf_api_token.trim().is_empty() || cfg.cf_zone_id.trim().is_empty() {
        anyhow::bail!("Cloudflare is enabled but API token or zone ID is missing");
    }

    let public_ip = if cfg.server_public_ip.trim().is_empty() {
        let ip = discover_public_ip().await?;
        let mut current = config::load()?;
        current.server_public_ip = ip.clone();
        config::save(&current)?;
        ip
    } else {
        cfg.server_public_ip.clone()
    };

    let endpoint = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        cfg.cf_zone_id
    );
    let payload = json!({
        "type": "A",
        "name": domain,
        "content": public_ip,
        "proxied": true,
        "comment": "Added via dasiiProxy v3"
    });

    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(endpoint)
        .bearer_auth(&cfg.cf_api_token)
        .json(&payload)
        .send()
        .await
        .context("Cloudflare DNS request failed")?;

    let status = response.status();
    let body: serde_json::Value = response.json().await
        .context("Cloudflare returned an invalid JSON response")?;
    if !status.is_success() || body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let message = body.get("errors").map(|v| v.to_string()).unwrap_or_default();
        anyhow::bail!("Cloudflare rejected DNS record (HTTP {status}): {message}");
    }
    Ok(true)
}

/// Sends a domain-scoped Discord error alert, limited to one alert per domain per 5 minutes.
pub async fn notify_proxy_error(cfg: &Config, domain: &str, error: &str) -> Result<()> {
    let webhook = cfg.discord_webhook.trim();
    if !webhook.starts_with("https://discord.com/api/webhooks/")
        && !webhook.starts_with("https://discordapp.com/api/webhooks/")
    {
        return Ok(());
    }

    let now = Instant::now();
    let cooldowns = ALERT_COOLDOWNS.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let mut entries = cooldowns.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.get(domain).is_some_and(|last| now.duration_since(*last) < Duration::from_secs(300)) {
            return Ok(());
        }
        entries.insert(domain.to_string(), now);
        if entries.len() > 2_000 {
            entries.retain(|_, last| now.duration_since(*last) < Duration::from_secs(600));
        }
    }

    let detail = error.chars().take(700).collect::<String>();
    let payload = json!({
        "content": "@here",
        "embeds": [{
            "title": "dasiiProxy v3 alert",
            "color": 16711680,
            "fields": [
                { "name": "Domain", "value": format!("`{domain}`"), "inline": true },
                { "name": "Time (UTC)", "value": chrono::Utc::now().to_rfc3339(), "inline": true },
                { "name": "Error", "value": format!("`{detail}`"), "inline": false }
            ],
            "footer": { "text": "dasiiProxy v3" }
        }]
    });

    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?
        .post(webhook)
        .json(&payload)
        .send()
        .await
        .context("Discord webhook request failed")?
        .error_for_status()
        .context("Discord webhook rejected the alert")?;
    Ok(())
}
