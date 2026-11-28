//! Outbound webhook delivery for server alerts.
//!
//! SSRF protection comes from [`crate::net::outbound_request`], which
//! refuses IP-literal hosts covered by `APP_BLOCKED_CIDRS` and resolves
//! hostnames through [`crate::net::BlockedIpResolver`], dropping every
//! resolved address in a blocked CIDR (including on redirect). Do not
//! deliver webhooks through `state.client` or any other client.
//!
//! Global API webhooks live here too: [`register_webhook_dispatchers`]
//! listens to the server, backup and user event emitters and delivers a
//! signed JSON envelope to every enabled [`WebhookSubscription`] that opted
//! into the event (see `WEBHOOK_EVENTS`). Delivery goes through
//! [`crate::net::outbound_request`] as well, and every attempt is recorded
//! in `webhook_deliveries`.

use crate::{
    State,
    models::{
        server::{Server, ServerEvent},
        server_alert::{ServerAlert, ServerAlertKind},
        server_backup::{ServerBackup, ServerBackupEvent},
        server_webhook::{ServerWebhook, ServerWebhookEvent, ServerWebhookKind},
        user::{User, UserEvent},
        webhook_delivery::{CreateWebhookDeliveryOptions, WebhookDelivery},
        webhook_subscription::WebhookSubscription,
    },
    prelude::*,
};
use futures_util::StreamExt;
use sha2::Digest;
use std::time::Duration;

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
        .map(|webhook| {
            // `payload` is moved into each delivery's async block; clone it
            // per webhook — the map closure runs many times (FnMut), so the
            // original cannot be moved.
            let payload = payload.clone();
            async move {
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
            }
        });

    futures_util::stream::iter(deliveries)
        .for_each_concurrent(4, |delivery| delivery)
        .await;
}


// ---------------------------------------------------------------------------
// Global API webhooks (event subscriptions)
// ---------------------------------------------------------------------------
// Signed HTTP delivery of panel events to admin-configured subscriptions.
//
// The dispatcher listens to the existing [`crate::events::EventEmitter`]s
// (see [`crate::models::EventEmittingModel`]) and POSTs a signed JSON
// envelope to every enabled subscription that opted into the event.
//
// Payload:
//
// ```json
// {
//   "event": "server.started",
//   "delivery_id": "00000000-0000-0000-0000-000000000000",
//   "sent_at": "2026-11-26T19:00:00+00:00",
//   "data": { "server_uuid": "…", "server_name": "…" }
// }
// ```
//
// Signing:
//
// The request carries `X-Calagopus-Event`, `X-Calagopus-Delivery`,
// `X-Calagopus-Timestamp` (unix seconds) and `X-Calagopus-Signature`.
// The signature is `sha256=<hex HMAC-SHA256(secret, "<timestamp>.<body>")>`,
// so a receiver can verify the payload with:
//
// ```text
// HMAC-SHA256(secret, timestamp + "." + raw_request_body)
// ```
//
// Delivery guarantees:
//
// Delivery is best-effort and never blocks the event that triggered it:
// each subscription is delivered in its own spawned task, with a 10 second
// request timeout and exactly one retry (after a 1 second delay) on any
// transport error or non-2xx response. Every attempt is recorded in
// `webhook_deliveries` so admins can see what was sent and what failed.

/// How long a single delivery attempt may take before it is aborted.
pub const WEBHOOK_DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);
/// Delay before the single retry of a failed delivery attempt.
pub const WEBHOOK_DELIVERY_RETRY_DELAY: Duration = Duration::from_secs(1);
/// Maximum number of attempts per delivery (first try + one retry).
pub const WEBHOOK_DELIVERY_MAX_ATTEMPTS: i32 = 2;

pub const WEBHOOK_SIGNATURE_HEADER: &str = "X-Calagopus-Signature";
pub const WEBHOOK_EVENT_HEADER: &str = "X-Calagopus-Event";
pub const WEBHOOK_TIMESTAMP_HEADER: &str = "X-Calagopus-Timestamp";
pub const WEBHOOK_DELIVERY_HEADER: &str = "X-Calagopus-Delivery";

/// Every event name a subscription may opt into.
pub const WEBHOOK_EVENTS: &[&str] = &[
    "server.created",
    "server.started",
    "server.stopped",
    "server.crashed",
    "server.install.started",
    "server.install.completed",
    "server.transfer.started",
    "server.transfer.completed",
    "backup.finished",
    "backup.restore.finished",
    "backup.deleted",
    "user.created",
];

#[inline]
pub fn is_valid_webhook_event(event: &str) -> bool {
    WEBHOOK_EVENTS.contains(&event)
}

fn server_payload(server: &Server) -> serde_json::Value {
    serde_json::json!({
        "server_uuid": server.uuid,
        "server_name": server.name,
        "server_external_id": server.external_id,
        "owner_uuid": server.owner.uuid,
        "node_uuid": server.node.uuid,
        "egg_uuid": server.egg.uuid,
    })
}

fn server_payload_with(
    server: &Server,
    extra: serde_json::Value,
) -> serde_json::Value {
    let mut payload = server_payload(server);

    if let (Some(payload), Some(extra)) = (payload.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            payload.insert(key.clone(), value.clone());
        }
    }

    payload
}

pub struct GlobalWebhookDeliveryOutcome {
    pub success: bool,
    pub status_code: Option<i32>,
    pub error: Option<String>,
    pub attempts: i32,
}

/// Delivers one event to one subscription, with a single retry, and records
/// every attempt in `webhook_deliveries`. The caller decides which
/// subscription/event pair to deliver; this function does not filter on the
/// subscription's event list (the test endpoint relies on that).
pub async fn deliver_to_subscription(
    state: &State,
    subscription: &WebhookSubscription,
    event: &str,
    data: serde_json::Value,
) -> Result<GlobalWebhookDeliveryOutcome, anyhow::Error> {
    let delivery_uuid = uuid::Uuid::new_v4();
    let timestamp = chrono::Utc::now().timestamp();
    let envelope = serde_json::json!({
        "event": event,
        "delivery_id": delivery_uuid,
        "sent_at": chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|sent_at| sent_at.to_rfc3339())
            .unwrap_or_default(),
        "data": data,
    });
    let body = serde_json::to_vec(&envelope)?;
    let secret = subscription.decrypt_secret(&state.database).await?;
    // Sign "<timestamp>.<body>" — the same construction as `sign_payload`,
    // inlined here because the timestamp is needed up front for the
    // envelope's `sent_at`.
    let mut signed = format!("{timestamp}.").into_bytes();
    signed.extend_from_slice(&body);
    let signature = format!("sha256={}", hmac_sha256_hex(secret.as_bytes(), &signed));

    let url = match reqwest::Url::parse(subscription.url.as_str()) {
        Ok(url) => url,
        Err(err) => {
            let error = format!("invalid subscription URL: {err}");
            WebhookDelivery::create(
                state,
                CreateWebhookDeliveryOptions {
                    subscription_uuid: subscription.uuid,
                    event: event.into(),
                    attempt: 1,
                    status_code: None,
                    success: false,
                    error: Some(error.clone().into()),
                    created: None,
                },
            )
            .await?;

            return Ok(GlobalWebhookDeliveryOutcome {
                success: false,
                status_code: None,
                error: Some(error),
                attempts: 1,
            });
        }
    };

    let mut attempt = 0;
    loop {
        attempt += 1;

        let (success, status_code, error) = match crate::net::outbound_request(
            &state.env,
            reqwest::Method::POST,
            url.clone(),
        ) {
            Ok(request) => {
                let response = tokio::time::timeout(
                    WEBHOOK_DELIVERY_TIMEOUT,
                    request
                        .header(WEBHOOK_EVENT_HEADER, event)
                        .header(WEBHOOK_DELIVERY_HEADER, delivery_uuid.to_string())
                        .header(WEBHOOK_TIMESTAMP_HEADER, timestamp.to_string())
                        .header(WEBHOOK_SIGNATURE_HEADER, signature.clone())
                        .header("Content-Type", "application/json")
                        .body(body.clone())
                        .send(),
                )
                .await;

                match response {
                    Ok(Ok(response)) => {
                        let status = response.status();

                        (
                            status.is_success(),
                            Some(status.as_u16() as i32),
                            if status.is_success() {
                                None
                            } else {
                                Some(format!("unexpected status code: {status}"))
                            },
                        )
                    }
                    Ok(Err(err)) => (false, None, Some(err.without_url().to_string())),
                    Err(_) => (
                        false,
                        None,
                        Some(format!(
                            "webhook delivery timed out after {WEBHOOK_DELIVERY_TIMEOUT:?}"
                        )),
                    ),
                }
            }
            // Refused by the SSRF guard before anything was connected.
            Err(err) => (false, None, Some(format!("blocked webhook URL: {err:#}"))),
        };

        WebhookDelivery::create(
            state,
            CreateWebhookDeliveryOptions {
                subscription_uuid: subscription.uuid,
                event: event.into(),
                attempt,
                status_code,
                success,
                error: error.clone().map(compact_str::CompactString::from),
                created: None,
            },
        )
        .await?;

        if success || attempt >= WEBHOOK_DELIVERY_MAX_ATTEMPTS {
            if !success {
                tracing::warn!(
                    subscription = %subscription.uuid,
                    event,
                    attempts = attempt,
                    status_code,
                    error = error.as_deref().unwrap_or("unknown error"),
                    "webhook delivery failed"
                );
            }

            return Ok(GlobalWebhookDeliveryOutcome {
                success,
                status_code,
                error,
                attempts: attempt,
            });
        }

        tokio::time::sleep(WEBHOOK_DELIVERY_RETRY_DELAY).await;
    }
}

/// Sends a `webhook.test` event to a subscription, bypassing its event list.
/// Used by the admin test endpoint.
pub async fn send_test_event(
    state: &State,
    subscription: &WebhookSubscription,
) -> Result<GlobalWebhookDeliveryOutcome, anyhow::Error> {
    deliver_to_subscription(
        state,
        subscription,
        "webhook.test",
        serde_json::json!({
            "subscription_uuid": subscription.uuid,
            "subscription_name": subscription.name,
            "url": subscription.url,
        }),
    )
    .await
}

/// Fans an event out to every enabled subscription that opted into it. Each
/// delivery runs in its own task, so a slow or dead endpoint can never block
/// the panel or the other subscribers.
pub async fn dispatch_event(state: &State, event: &str, data: serde_json::Value) {
    let subscriptions = match WebhookSubscription::all_enabled_for_event(&state.database, event)
        .await
    {
        Ok(subscriptions) => subscriptions,
        Err(err) => {
            tracing::warn!(event, "failed to load webhook subscriptions: {err:#}");
            return;
        }
    };

    for subscription in subscriptions {
        let state = state.clone();
        let event = event.to_string();

        tokio::spawn(async move {
            if let Err(err) = deliver_to_subscription(&state, &subscription, &event, data).await {
                tracing::warn!(
                    subscription = %subscription.uuid,
                    event = %event,
                    "webhook delivery failed: {err:#}"
                );
            }
        });
    }
}

/// Registers the webhook dispatcher on the server, backup and user event
/// emitters. Call once at startup (see `backend::handle_startup`).
///
/// The returned handles intentionally do not need to be kept alive:
/// [`crate::events::EventHandlerHandle::disconnect`] is explicit, so dropping
/// them leaves the listeners registered for the life of the process.
pub fn register_webhook_dispatchers(_state: &State) -> Vec<crate::events::EventHandlerHandle> {
    let server_handle = Server::register_event_handler(|state, event| async move {
        let (name, data) = match &*event {
            ServerEvent::Created { server } => ("server.created", server_payload(server)),
            ServerEvent::Started { server } => ("server.started", server_payload(server)),
            ServerEvent::Stopped { server } => ("server.stopped", server_payload(server)),
            ServerEvent::Crashed { server } => ("server.crashed", server_payload(server)),
            ServerEvent::InstallStarted { server, .. } => {
                ("server.install.started", server_payload(server))
            }
            ServerEvent::InstallCompleted { server, successful } => (
                "server.install.completed",
                server_payload_with(
                    server,
                    serde_json::json!({ "successful": successful }),
                ),
            ),
            ServerEvent::TransferStarted {
                server,
                destination_node,
                ..
            } => (
                "server.transfer.started",
                server_payload_with(
                    server,
                    serde_json::json!({ "destination_node_uuid": destination_node.uuid }),
                ),
            ),
            ServerEvent::TransferCompleted {
                server,
                successful,
                ..
            } => (
                "server.transfer.completed",
                server_payload_with(
                    server,
                    serde_json::json!({ "successful": successful }),
                ),
            ),
            ServerEvent::StateReset { server } => {
                // A manual state reset is not a power event; nothing to send.
                let _ = server;
                return Ok(());
            }
        };

        dispatch_event(&state, name, data).await;

        Ok(())
    });

    let backup_handle = ServerBackup::register_event_handler(|state, event| async move {
        let (name, data) = match &*event {
            ServerBackupEvent::CreationCompleted { backup, successful } => (
                "backup.finished",
                serde_json::json!({
                    "backup_uuid": backup.uuid,
                    "server_uuid": backup.server.as_ref().map(|server| server.uuid),
                    "successful": successful,
                }),
            ),
            ServerBackupEvent::RestoreCompleted {
                backup,
                server,
                successful,
            } => (
                "backup.restore.finished",
                serde_json::json!({
                    "backup_uuid": backup.uuid,
                    "server_uuid": server.uuid,
                    "successful": successful,
                }),
            ),
            ServerBackupEvent::DeletionCompleted { backup, successful } => (
                "backup.deleted",
                serde_json::json!({
                    "backup_uuid": backup.uuid,
                    "server_uuid": backup.server.as_ref().map(|server| server.uuid),
                    "successful": successful,
                }),
            ),
            ServerBackupEvent::RestoreStarted { .. } => return Ok(()),
        };

        dispatch_event(&state, name, data).await;

        Ok(())
    });

    let user_handle = User::register_event_handler(|state, event| async move {
        let UserEvent::Created { user } = &*event;

        dispatch_event(
            &state,
            "user.created",
            serde_json::json!({
                "user_uuid": user.uuid,
                "username": user.username,
                "email": user.email,
            }),
        )
        .await;

        Ok(())
    });

    vec![server_handle, backup_handle, user_handle]
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

    #[test]
    fn global_webhook_signature_is_stable_and_bound_to_timestamp_and_body() {
        // Same construction as `deliver_to_subscription`: HMAC-SHA256 over
        // "<timestamp>.<body>".
        let body = br#"{"event":"server.started"}"#;
        let sign = |timestamp: i64| {
            let mut signed = format!("{timestamp}.").into_bytes();
            signed.extend_from_slice(body);
            hmac_sha256_hex(b"secret", &signed)
        };

        assert_eq!(sign(1_700_000_000), sign(1_700_000_000));
        assert_ne!(sign(1_700_000_000), sign(1_700_000_001));
        assert_ne!(
            hmac_sha256_hex(b"secret", body),
            hmac_sha256_hex(b"other", body)
        );
    }

    #[test]
    fn global_webhook_event_catalogue_covers_the_documented_events() {
        for event in [
            "server.created",
            "server.started",
            "server.stopped",
            "server.crashed",
            "backup.finished",
            "user.created",
        ] {
            assert!(is_valid_webhook_event(event), "{event}");
        }

        assert!(!is_valid_webhook_event("server.exploded"));
    }
}
