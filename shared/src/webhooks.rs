//! Outbound webhook delivery for server alerts.
//!
//! SSRF protection comes from [`crate::net::outbound_request`], which
//! refuses IP-literal hosts covered by `APP_BLOCKED_CIDRS` and resolves
//! hostnames through [`crate::net::BlockedIpResolver`], dropping every
//! resolved address in a blocked CIDR (including on redirect). Do not
//! deliver webhooks through `state.client` or any other client.

use crate::{
    State,
    models::{
        server::Server,
        server_alert::{ServerAlert, ServerAlertKind},
        server_webhook::{ServerWebhook, ServerWebhookEvent, ServerWebhookKind},
    },
};
use futures_util::StreamExt;
use sha2::Digest;

/// How long a single delivery attempt may take.
pub const WEBHOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Delay between the first attempt and the single retry.
pub const WEBHOOK_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// Console tail kept on the alert row and sent to webhooks.
pub const MAX_CONSOLE_CHARS: usize = 4000;
/// Console shown inside a Discord embed code block (Discord's field
/// limit is 1024 characters; the embed description holds the rest).
pub const MAX_DISCORD_CONSOLE_CHARS: usize = 900;

/// The event payload delivered to a generic webhook, and the data a
/// Discord embed is built from.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookPayload {
    pub event: &'static str,
    pub server_uuid: uuid::Uuid,
    pub server_uuid_short: String,
    pub server_name: String,
    pub detected_at: chrono::DateTime<chrono::Utc>,
    pub crash_id: Option<i64>,
    pub exit_code: Option<i32>,
    pub oom_killed: bool,
    /// Last console lines at detection time (newest last), if the
    /// watcher could fetch them from Wings.
    pub console_lines: Vec<String>,
}

impl WebhookPayload {
    pub fn from_alert(server: &Server, alert: &ServerAlert) -> Self {
        Self {
            event: match alert.kind {
                ServerAlertKind::Crashed => "server.crashed",
                ServerAlertKind::Offline => "server.offline",
                ServerAlertKind::Recovered => "server.recovered",
            },
            server_uuid: server.uuid,
            server_uuid_short: format!("{:08x}", server.uuid_short),
            server_name: server.name.to_string(),
            detected_at: alert.detected_at.and_utc(),
            crash_id: alert.crash_id,
            exit_code: alert.exit_code,
            oom_killed: alert.oom_killed,
            console_lines: alert
                .last_console
                .as_ref()
                .map(|console| console.lines().map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

pub struct WebhookDeliveryOutcome {
    pub successful: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
}

/// Rejects a webhook URL at creation/update time.
///
/// * must be `https` (webhook URLs carry bearer tokens; plain HTTP
///   would leak them, and Discord requires HTTPS anyway),
/// * must have a host, no embedded credentials,
/// * must not be an IP literal covered by `APP_BLOCKED_CIDRs`,
/// * `localhost` is rejected outright (it resolves to a blocked CIDR
///   at delivery time, but failing fast gives a better error).
///
/// Resolution-time blocking still happens on every delivery via
/// [`crate::net::outbound_request`]: a hostname that resolves to a
/// private address after creation is refused then (DNS-rebinding
/// safe), so this check is a UX guard, not the security boundary.
pub fn validate_webhook_url(
    env: &std::sync::Arc<crate::env::Env>,
    url: &str,
) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(url).map_err(|_| "invalid URL".to_string())?;

    if url.scheme() != "https" {
        return Err("webhook URL must use https".to_string());
    }
    if url.username() != "" || url.password().is_some() {
        return Err("webhook URL must not contain credentials".to_string());
    }
    let Some(host) = url.host_str() else {
        return Err("webhook URL must have a host".to_string());
    };
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err("webhook URL must not point at localhost".to_string());
    }
    if let Some(ip) = crate::net::host_to_ip(host)
        && crate::net::is_blocked_ip(&env.app_blocked_cidrs, &ip)
    {
        return Err("webhook URL resolves to a blocked IP address".to_string());
    }

    Ok(url)
}

/// Discord webhooks live under `discord.com/api/webhooks/…` (or the
/// legacy `discordapp.com` host). A `discord` webhook pointed anywhere
/// else is a misconfiguration, not a Discord webhook.
pub fn validate_discord_url(url: &reqwest::Url) -> Result<(), String> {
    let host_ok = matches!(
        url.host_str(),
        Some("discord.com") | Some("discordapp.com") | Some("ptb.discord.com") | Some("canary.discord.com")
    );
    if !host_ok || !url.path().starts_with("/api/webhooks/") {
        return Err("discord webhook URL must be a discord.com/api/webhooks/ URL".to_string());
    }

    Ok(())
}

/// HMAC-SHA256, implemented on `sha2` so no new dependency is needed.
/// `hmac` is not currently in the workspace dependency set.
pub fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;

    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let digest = sha2::Sha256::digest(key);
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0u8; BLOCK_SIZE];
    let mut outer_pad = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        inner_pad[i] = key_block[i] ^ 0x36;
        outer_pad[i] = key_block[i] ^ 0x5c;
    }

    let mut inner = sha2::Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = sha2::Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);

    hex::encode(outer.finalize())
}

/// Signs `body` as `"{timestamp}.{body}"` under `secret`.
/// Returns `(signature_hex, timestamp_unix)`.
pub fn sign_payload(secret: &str, body: &[u8]) -> (String, i64) {
    let timestamp = chrono::Utc::now().timestamp();
    let message = format!("{timestamp}.");
    let mut signed = message.into_bytes();
    signed.extend_from_slice(body);

    (hmac_sha256_hex(secret.as_bytes(), &signed), timestamp)
}

fn truncate_console(console: &str, max_chars: usize) -> String {
    if console.len() <= max_chars {
        return console.to_string();
    }

    // Keep the tail: the last lines are what explain a crash.
    let tail = &console[console.len() - max_chars..];
    let tail = tail.split_once('\n').map(|(_, rest)| rest).unwrap_or(tail);
    format!("…{tail}")
}

fn discord_embed(payload: &WebhookPayload) -> serde_json::Value {
    let title = match payload.event {
        "server.crashed" => format!("💥 Server crashed: {}", payload.server_name),
        "server.offline" => format!("⚠️ Server went offline: {}", payload.server_name),
        _ => format!("✅ Server recovered: {}", payload.server_name),
    };
    let color = match payload.event {
        "server.crashed" => 0xED42_45,
        "server.offline" => 0xFEE7_5C,
        _ => 0x57F2_87,
    };

    let mut fields = vec![
        serde_json::json!({
            "name": "Server",
            "value": format!("{} (`{}`)", payload.server_name, payload.server_uuid_short),
            "inline": true,
        }),
        serde_json::json!({
            "name": "Detected",
            "value": format!("<t:{}:R>", payload.detected_at.timestamp()),
            "inline": true,
        }),
    ];
    if let Some(exit_code) = payload.exit_code {
        fields.push(serde_json::json!({
            "name": "Exit code",
            "value": exit_code.to_string(),
            "inline": true,
        }));
    }
    if payload.oom_killed {
        fields.push(serde_json::json!({
            "name": "OOM killed",
            "value": "Yes — the server ran out of memory",
            "inline": true,
        }));
    }
    if let Some(crash_id) = payload.crash_id {
        fields.push(serde_json::json!({
            "name": "Crash",
            "value": format!("`#{crash_id}` — see Console → Crashes in the panel"),
            "inline": true,
        }));
    }

    let mut embed = serde_json::json!({
        "title": title,
        "color": color,
        "fields": fields,
        "timestamp": payload.detected_at.to_rfc3339(),
    });

    if !payload.console_lines.is_empty() {
        let console = truncate_console(
            &payload.console_lines.join("\n"),
            MAX_DISCORD_CONSOLE_CHARS,
        );
        embed["description"] = serde_json::json!(format!("```\n{console}\n```"));
    }

    serde_json::json!({ "embeds": [embed] })
}

/// Delivers `payload` to one webhook, retrying once on a transport
/// error or a 5xx response. 4xx responses are not retried: the URL or
/// payload is wrong, and retrying would only spam a dead endpoint.
///
/// Returns the outcome; the caller records it on the webhook row.
/// Delivery failure never fails the alert itself — the alert row and
/// the in-panel activity entry are the durable record.
pub async fn dispatch_webhook(
    state: &State,
    webhook: &ServerWebhook,
    payload: &WebhookPayload,
) -> WebhookDeliveryOutcome {
    let url = match reqwest::Url::parse(webhook.url.as_str()) {
        Ok(url) => url,
        Err(err) => {
            return WebhookDeliveryOutcome {
                successful: false,
                status: None,
                error: Some(format!("invalid stored webhook URL: {err}")),
            };
        }
    };

    let (body, signature): (serde_json::Value, Option<(String, i64)>) = match webhook.kind {
        ServerWebhookKind::Discord => (discord_embed(payload), None),
        ServerWebhookKind::Generic => {
            let body = serde_json::to_value(payload).unwrap_or(serde_json::json!({}));
            let signature = match &webhook.secret {
                Some(secret) => match secret.decrypt(&state.database).await {
                    Ok(secret) => Some(sign_payload(
                        secret.as_str(),
                        serde_json::to_vec(&body).unwrap_or_default().as_slice(),
                    )),
                    Err(err) => {
                        return WebhookDeliveryOutcome {
                            successful: false,
                            status: None,
                            error: Some(format!("failed to decrypt webhook secret: {err:#}")),
                        };
                    }
                },
                None => None,
            };
            (body, signature)
        }
    };

    let mut last_error: Option<String> = None;

    for attempt in 0..2 {
        if attempt == 1 {
            tokio::time::sleep(WEBHOOK_RETRY_DELAY).await;
        }

        let request = match crate::net::outbound_request(&state.env, reqwest::Method::POST, url.clone())
        {
            Ok(request) => request,
            Err(err) => {
                return WebhookDeliveryOutcome {
                    successful: false,
                    status: None,
                    error: Some(format!("blocked webhook URL: {err:#}")),
                };
            }
        };

        let mut request = request
            .header("Content-Type", "application/json")
            .header("User-Agent", format!("Calagopus-Panel/{}", crate::VERSION))
            .json(&body);
        if let Some((signature, timestamp)) = &signature {
            request = request
                .header("X-Calagopus-Event", payload.event)
                .header("X-Calagopus-Signature", format!("sha256={signature}"))
                .header("X-Calagopus-Timestamp", timestamp.to_string());
        }

        let response = tokio::time::timeout(WEBHOOK_TIMEOUT, request.send()).await;

        match response {
            Ok(Ok(response)) => {
                let status = response.status();
                if status.is_success() {
                    return WebhookDeliveryOutcome {
                        successful: true,
                        status: Some(status.as_u16()),
                        error: None,
                    };
                }
                if status.is_client_error() {
                    return WebhookDeliveryOutcome {
                        successful: false,
                        status: Some(status.as_u16()),
                        error: Some(format!("webhook rejected the payload: {status}")),
                    };
                }
                last_error = Some(format!("webhook returned {status}"));
            }
            Ok(Err(err)) => last_error = Some(format!("webhook request failed: {err:#}")),
            Err(_) => last_error = Some(format!("webhook request timed out after {WEBHOOK_TIMEOUT:?}")),
        }
    }

    WebhookDeliveryOutcome {
        successful: false,
        status: None,
        error: last_error,
    }
}

/// Delivers `payload` to every enabled webhook for the server (its
/// own, plus the global ones) whose event list includes this event,
/// concurrently. Each delivery is recorded on its webhook row
/// (`last_triggered_at`, `failure_count`); failures are logged, not
/// raised, so one dead URL cannot block the others.
pub async fn dispatch_server_alert(
    state: &State,
    server: &Server,
    alert: &ServerAlert,
    webhooks: &[ServerWebhook],
) {
    let payload = WebhookPayload::from_alert(server, alert);
    let event = match alert.kind {
        ServerAlertKind::Crashed => ServerWebhookEvent::ServerCrashed,
        ServerAlertKind::Offline => ServerWebhookEvent::ServerOffline,
        ServerAlertKind::Recovered => ServerWebhookEvent::ServerRecovered,
    };

    let deliveries = webhooks
        .iter()
        .filter(|webhook| webhook.enabled && webhook.events.contains(&event))
        .map(|webhook| async move {
            let outcome = dispatch_webhook(state, webhook, &payload).await;

            if let Err(err) =
                ServerWebhook::record_delivery(&state.database, webhook.uuid, outcome.successful)
                    .await
            {
                tracing::warn!(
                    webhook = %webhook.uuid,
                    "failed to record webhook delivery: {err:#?}"
                );
            }
            if !outcome.successful {
                tracing::warn!(
                    webhook = %webhook.uuid,
                    server = %server.uuid,
                    event = payload.event,
                    error = outcome.error.as_deref().unwrap_or("unknown error"),
                    "webhook delivery failed for server alert"
                );
            }
        });

    futures_util::stream::iter(deliveries)
        .for_each_concurrent(4, |delivery| delivery)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 4231 test case 1: key 0x0b * 20, data "Hi There".
    #[test]
    fn hmac_sha256_matches_rfc_4231_test_case_1() {
        let signature = hmac_sha256_hex(&[0x0b; 20], b"Hi There");

        assert_eq!(
            signature,
            "b0344c61d8db3853ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn truncate_console_keeps_the_tail() {
        let console = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let truncated = truncate_console(&console, 20);

        assert!(truncated.starts_with('…'));
        assert!(truncated.contains("line 99"));
        assert!(!truncated.contains("line 0\n"));
    }

    #[test]
    fn validate_discord_url_requires_discord_webhook_host_and_path() {
        let good = reqwest::Url::parse("https://discord.com/api/webhooks/123/abc").unwrap();
        assert!(validate_discord_url(&good).is_ok());

        let wrong_host = reqwest::Url::parse("https://example.com/api/webhooks/123/abc").unwrap();
        assert!(validate_discord_url(&wrong_host).is_err());

        let wrong_path = reqwest::Url::parse("https://discord.com/api/other").unwrap();
        assert!(validate_discord_url(&wrong_path).is_err());
    }
}
