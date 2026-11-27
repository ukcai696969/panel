// Billing helpers shared by the billing API routes and the hourly task.
//
// STRIPE IS A STUB IN THIS FIRST VERSION. There is deliberately no live
// Stripe API call anywhere in this file:
//   * `create_checkout_session_stub` only mints a local `cs_test_stub_…`
//     session id and returns a checkout URL built from the resolved
//     checkout URL template (settings, or the
//     `BILLING_CHECKOUT_URL_TEMPLATE` environment variable). No money
//     moves until a real Stripe integration replaces this function with
//     a call to Stripe's Checkout Sessions API (test mode first) and the
//     webhook below starts receiving real `checkout.session.completed`
//     events.
//   * The webhook DOES verify the Stripe signature (HMAC-SHA256, Stripe's
//     `t=…,v1=…` header format) when `STRIPE_WEBHOOK_SECRET` is set (in
//     settings or the environment), and rejects everything when it is
//     not set. That verification is real and must stay in place when
//     the stub is replaced.
//   * Live keys are refused: a secret key must start with `sk_test_`.
//     A `sk_live_` key is an error, never a charge.
//
// Money: 1 credit = 1 minor currency unit. See `shared::settings::billing`.

use chrono::Timelike;
use sha2::{Digest, Sha256};
use shared::settings::billing::AppSettingsBilling;
use shared::models::{
    CreatableModel,
    server::Server,
    server_activity::{CreateServerActivityOptions, ServerActivity},
    server_billing_state::ServerBillingState,
    user_credit::UserCredit,
    user_credit_transaction::UserCreditTransactionKind,
};

/// Stripe configuration, resolved at request time.
///
/// Precedence, per value (highest first):
///   1. the matching environment variable, when set and non-empty
///      (`STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`,
///      `STRIPE_PUBLISHABLE_KEY`, `BILLING_CHECKOUT_URL_TEMPLATE`);
///   2. the stored settings value (`AppSettingsBilling` — the secret
///      key and webhook secret are encrypted at rest, the publishable
///      key and checkout URL template are not secret);
///   3. the built-in default (checkout URL template only).
///
/// An environment variable that is set but empty (or whitespace-only)
/// counts as unset. Settings are live (60 s cache, see
/// `shared::settings`), so a saved key takes effect without a restart
/// — unless the matching env var is set, in which case the saved value
/// is shadowed until the env var is removed. Returns `None` when no
/// secret key resolves from either source: Stripe is not configured.
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: Option<String>,
    pub publishable_key: Option<String>,
    /// Resolved template; never empty (falls back to
    /// [`AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE`]).
    pub checkout_url_template: String,
}

impl StripeConfig {
    /// An environment variable's value, ignoring unset, empty and
    /// whitespace-only values.
    fn env_var(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.trim().is_empty())
    }

    /// Environment only. Kept for callers/tests that need the env
    /// view; request handlers should use [`Self::resolve`] instead.
    pub fn from_env() -> Option<Self> {
        let secret_key = Self::env_var("STRIPE_SECRET_KEY")?;

        Some(Self {
            secret_key,
            webhook_secret: Self::env_var("STRIPE_WEBHOOK_SECRET"),
            publishable_key: Self::env_var("STRIPE_PUBLISHABLE_KEY"),
            checkout_url_template: Self::env_var("BILLING_CHECKOUT_URL_TEMPLATE")
                .unwrap_or_else(|| AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE.to_string()),
        })
    }

    /// Stored settings only, ignoring the environment.
    pub fn from_billing(billing: &AppSettingsBilling) -> Option<Self> {
        let secret_key = billing.stripe_secret_key.as_ref()?.to_string();
        if secret_key.trim().is_empty() {
            return None;
        }

        Some(Self {
            secret_key,
            webhook_secret: billing
                .stripe_webhook_secret
                .as_ref()
                .map(|secret| secret.to_string()),
            publishable_key: billing
                .stripe_publishable_key
                .as_ref()
                .map(|key| key.to_string()),
            checkout_url_template: if billing.checkout_url_template.trim().is_empty() {
                AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE.to_string()
            } else {
                billing.checkout_url_template.to_string()
            },
        })
    }

    /// Settings merged with the environment: env wins per value when
    /// set, otherwise the stored settings value is used.
    pub fn resolve_from(billing: &AppSettingsBilling) -> Option<Self> {
        let secret_key = Self::env_var("STRIPE_SECRET_KEY").or_else(|| {
            billing
                .stripe_secret_key
                .as_ref()
                .map(|secret| secret.to_string())
        })?;
        if secret_key.trim().is_empty() {
            return None;
        }

        Some(Self {
            secret_key,
            webhook_secret: Self::env_var("STRIPE_WEBHOOK_SECRET").or_else(|| {
                billing
                    .stripe_webhook_secret
                    .as_ref()
                    .map(|secret| secret.to_string())
            }),
            publishable_key: Self::env_var("STRIPE_PUBLISHABLE_KEY").or_else(|| {
                billing
                    .stripe_publishable_key
                    .as_ref()
                    .map(|key| key.to_string())
            }),
            checkout_url_template: Self::env_var("BILLING_CHECKOUT_URL_TEMPLATE")
                .or_else(|| Some(billing.checkout_url_template.to_string()))
                .filter(|template| !template.trim().is_empty())
                .unwrap_or_else(|| AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE.to_string()),
        })
    }

    /// Resolve from the live settings, with the environment as an
    /// override per value. This is what request handlers use.
    pub async fn resolve(
        settings: &shared::settings::Settings,
    ) -> Result<Option<Self>, anyhow::Error> {
        let billing = settings.get_as(|s| s.billing.clone()).await?;
        Ok(Self::resolve_from(&billing))
    }

    /// Test mode only in v1. A live key must never reach the stub: it
    /// would imply real charges are possible, and they are not.
    pub fn require_test_mode(&self) -> Result<(), anyhow::Error> {
        if self.secret_key.starts_with("sk_test_") {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "billing is test-mode only: STRIPE_SECRET_KEY (settings or environment) must start with sk_test_"
            ))
        }
    }
}

/// What the client needs to send the user to Stripe (or, in v1, to the
/// stub checkout URL). `session_id` is the idempotency reference: the
/// webhook credits the purchase exactly once per session id.
pub struct CheckoutSession {
    pub session_id: String,
    pub checkout_url: String,
}

/// STUB — see the module docs. Mints a local test session id and builds
/// the checkout URL from `checkout_url_template` (the resolved template:
/// settings `billing::checkout_url_template`, overridden by
/// `BILLING_CHECKOUT_URL_TEMPLATE` — see [`StripeConfig::resolve`]),
/// which may use `{session_id}`, `{credits}` and `{app_url}`. Default
/// template sends the user back to their billing page with the session
/// id attached, so the flow can be exercised end-to-end without Stripe.
pub fn create_checkout_session_stub(
    app_url: &str,
    credits: i64,
    currency: &str,
    checkout_url_template: &str,
) -> Result<CheckoutSession, anyhow::Error> {
    let _ = currency;
    let session_id = format!("cs_test_stub_{}", uuid::Uuid::new_v4());

    let template = if checkout_url_template.trim().is_empty() {
        AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE
    } else {
        checkout_url_template
    };

    let checkout_url = template
        .replace("{session_id}", &session_id)
        .replace("{credits}", &credits.to_string())
        .replace("{app_url}", app_url.trim_end_matches('/'));

    Ok(CheckoutSession {
        session_id,
        checkout_url,
    })
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // HMAC-SHA256 implemented directly on `sha2` so no new crate is
    // needed. Standard construction: H(K XOR opad, H(K XOR ipad, text)).
    const BLOCK_SIZE: usize = 64;

    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0u8; BLOCK_SIZE];
    let mut outer_pad = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        inner_pad[i] = key_block[i] ^ 0x36;
        outer_pad[i] = key_block[i] ^ 0x5c;
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    outer.finalize().into()
}

/// Verify a Stripe webhook signature header (`t=<unix>,v1=<hex>,…`).
/// Returns false for a missing/old header, a bad timestamp, or a
/// signature mismatch. Timestamp tolerance is Stripe's 5 minutes.
pub fn verify_stripe_signature(payload: &[u8], signature_header: &str, secret: &str) -> bool {
    let mut timestamp: Option<i64> = None;
    let mut signatures: Vec<&str> = Vec::new();

    for part in signature_header.split(',') {
        if let Some(value) = part.strip_prefix("t=") {
            timestamp = value.parse().ok();
        } else if let Some(value) = part.strip_prefix("v1=") {
            signatures.push(value);
        }
    }

    let Some(timestamp) = timestamp else {
        return false;
    };

    let now = chrono::Utc::now().timestamp();
    if (now - timestamp).abs() > 300 {
        return false;
    }

    let signed_payload = format!("{timestamp}.{}", String::from_utf8_lossy(payload));
    let expected = hmac_sha256(secret.as_bytes(), signed_payload.as_bytes());
    let expected_hex = hex::encode(expected);

    signatures.iter().any(|signature| {
        constant_time_eq::constant_time_eq(expected_hex.as_bytes(), signature.as_bytes())
    })
}

pub struct HourlyChargeOutcome {
    pub servers_checked: u64,
    pub servers_charged: u64,
    pub servers_suspended: u64,
    pub credits_charged: i64,
}

/// Charge every chargeable server for the current hour.
///
/// A server is chargeable when billing is enabled, it is not already
/// suspended, and it has no in-progress panel status (installing /
/// restoring — `Server.status` is `Some` while those run). Each server
/// is charged inside its own transaction, locked on its billing-state
/// row, and stamped with the hour it was charged for, so a second run
/// in the same hour is a no-op for that server.
///
/// Suspension reuses the existing `servers.suspended` flag only — no
/// new Wings call is invented here. A suspended server stops being
/// charged (it is skipped above), and it is NOT automatically
/// unsuspended when the balance recovers; an admin unsuspends it.
pub async fn charge_running_servers(
    state: &shared::State,
) -> Result<HourlyChargeOutcome, anyhow::Error> {
    let billing = state.settings.get_as(|s| s.billing.clone()).await?;
    if !billing.enabled {
        return Ok(HourlyChargeOutcome {
            servers_checked: 0,
            servers_charged: 0,
            servers_suspended: 0,
            credits_charged: 0,
        });
    }

    let now = chrono::Utc::now().naive_utc();
    // Truncate to the hour: the idempotency stamp for this charge.
    let hour = now
        .date()
        .and_hms_opt(now.hour(), 0, 0)
        .unwrap_or(now);

    let mut outcome = HourlyChargeOutcome {
        servers_checked: 0,
        servers_charged: 0,
        servers_suspended: 0,
        credits_charged: 0,
    };

    let mut page = 1;
    loop {
        let servers = Server::all_with_pagination(&state.database, page, 100, None).await?;
        if servers.data.is_empty() {
            break;
        }

        for server in &servers.data {
            outcome.servers_checked += 1;

            if server.suspended || server.status.is_some() {
                continue;
            }

            let cost = billing.hourly_cost_credits(server.memory, server.cpu, server.disk);
            if cost <= 0 {
                continue;
            }

            let mut transaction = state.database.write().begin().await?;
            let mut billing_state =
                ServerBillingState::get_or_create_locked(server.uuid, &mut transaction).await?;

            if billing_state.last_charged_at == Some(hour) {
                transaction.rollback().await?;
                continue;
            }

            let (credit, _) = match UserCredit::apply_with_transaction(
                state,
                server.owner.uuid,
                -cost,
                UserCreditTransactionKind::Charge,
                Some(server.uuid),
                Some(format!("hourly:{hour}").into()),
                Some(
                    format!(
                        "Hourly charge for server {} ({})",
                        server.name, server.uuid
                    )
                    .into(),
                ),
                &mut transaction,
            )
            .await
            {
                Ok(result) => result,
                Err(err) => {
                    transaction.rollback().await?;
                    tracing::warn!(
                        server = %server.uuid,
                        "failed to charge server for billing hour: {err:#?}"
                    );
                    continue;
                }
            };

            if let Err(err) = billing_state
                .mark_charged(hour, cost, &mut transaction)
                .await
            {
                transaction.rollback().await?;
                tracing::warn!(
                    server = %server.uuid,
                    "failed to record billing charge for server: {err:#?}"
                );
                continue;
            }

            if let Err(err) = transaction.commit().await {
                tracing::warn!(
                    server = %server.uuid,
                    "failed to commit billing charge for server: {err:#?}"
                );
                continue;
            }

            outcome.servers_charged += 1;
            outcome.credits_charged += cost;

            if billing.suspend_on_negative_balance && credit.balance_credits < 0 {
                let suspended = sqlx::query(
                    r#"
                    UPDATE servers
                    SET suspended = true
                    WHERE servers.uuid = $1 AND servers.suspended = false
                    "#,
                )
                .bind(server.uuid)
                .execute(state.database.write())
                .await?;

                if suspended.rows_affected() == 1 {
                    outcome.servers_suspended += 1;

                    if let Err(err) = ServerActivity::create(
                        state,
                        CreateServerActivityOptions {
                            server_uuid: server.uuid,
                            user_uuid: None,
                            impersonator_uuid: None,
                            api_key_uuid: None,
                            schedule_uuid: None,
                            event: "server:billing.suspended".into(),
                            ip: None,
                            data: serde_json::json!({
                                "balance_credits": credit.balance_credits,
                                "hourly_cost_credits": cost,
                                "hour": hour.to_string(),
                            }),
                            created: None,
                        },
                    )
                    .await
                    {
                        tracing::warn!(
                            server = %server.uuid,
                            "failed to log billing suspension activity: {err:#?}"
                        );
                    }

                    tracing::info!(
                        server = %server.uuid,
                        user = %server.owner.uuid,
                        balance_credits = credit.balance_credits,
                        "suspended server: credit balance below zero"
                    );
                }
            }
        }

        if (servers.data.len() as i64) < servers.per_page {
            break;
        }
        page += 1;
    }

    Ok(outcome)
}
