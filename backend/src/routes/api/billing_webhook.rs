// Stripe webhook — public route, NO session/API-key auth.
//
// Mounted at `/api/billing-webhook` in `backend/src/routes/api/mod.rs`,
// deliberately outside `/api/client` and `/api/admin` so Stripe can
// reach it without a panel login.
//
// Security, in order:
//   1. `STRIPE_WEBHOOK_SECRET` must be set (in settings or the
//      environment — see `crate::billing::StripeConfig::resolve`), or
//      every request is 403.
//   2. The `Stripe-Signature` header must verify (HMAC-SHA256, see
//      `crate::billing::verify_stripe_signature`), or the request is 400.
//   3. A purchase is credited exactly once per Stripe checkout session
//      id (`UserCreditTransaction::exists_by_reference`), so Stripe
//      retries / replays cannot double-credit.
//   4. Only `checkout.session.completed` is acted on; every other
//      event type is acknowledged with 200 and ignored.
//
// The session's `metadata` must carry `user_uuid` and `credits` — the
// real Checkout Session creation (which replaces the stub in
// `crate::billing`) must set those. A completed session without them
// is logged and ignored, never credited to a guessed user.
//
// This endpoint intentionally does NOT use the utoipa `routes!` macro:
// Stripe sends a raw body whose signature is computed over the exact
// bytes, so the handler takes `axum::body::Bytes` directly.

use super::State;
use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use shared::{
    GetState,
    models::{
        user_credit::UserCredit,
        user_credit_transaction::{UserCreditTransaction, UserCreditTransactionKind},
    },
    response::{ApiResponse, ApiResponseResult},
};
use utoipa_axum::router::OpenApiRouter;

#[derive(Deserialize)]
struct StripeEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: StripeEventData,
}

#[derive(Deserialize)]
struct StripeEventData {
    object: StripeCheckoutSession,
}

#[derive(Deserialize)]
struct StripeCheckoutSession {
    id: String,
    metadata: Option<StripeSessionMetadata>,
}

#[derive(Deserialize)]
struct StripeSessionMetadata {
    user_uuid: Option<String>,
    credits: Option<String>,
}

pub async fn route(state: GetState, headers: HeaderMap, body: Bytes) -> ApiResponseResult {
    // Resolved from settings with the environment as an override,
    // so a saved webhook secret takes effect without a restart.
    let Some(config) = crate::billing::StripeConfig::resolve(&state.settings).await? else {
        return ApiResponse::error("stripe is not configured")
            .with_status(StatusCode::FORBIDDEN)
            .ok();
    };
    let Some(webhook_secret) = config.webhook_secret else {
        return ApiResponse::error("stripe webhook is not configured")
            .with_status(StatusCode::FORBIDDEN)
            .ok();
    };

    let Some(signature) = headers
        .get("Stripe-Signature")
        .and_then(|value| value.to_str().ok())
    else {
        return ApiResponse::error("missing stripe signature")
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
    };

    if !crate::billing::verify_stripe_signature(&body, signature, &webhook_secret) {
        tracing::warn!("rejected stripe webhook with invalid signature");
        return ApiResponse::error("invalid stripe signature")
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
    }

    let event: StripeEvent = match serde_json::from_slice(&body) {
        Ok(event) => event,
        Err(err) => {
            tracing::warn!("rejected stripe webhook with invalid json: {err:#?}");
            return ApiResponse::error("invalid webhook payload")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }
    };

    if event.event_type != "checkout.session.completed" {
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    }

    let session = event.data.object;
    let Some(metadata) = session.metadata else {
        tracing::warn!(
            session = %session.id,
            "stripe checkout completed without metadata, not crediting"
        );
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    };

    let (Some(user_uuid), Some(credits)) = (metadata.user_uuid, metadata.credits) else {
        tracing::warn!(
            session = %session.id,
            "stripe checkout completed without user_uuid/credits metadata, not crediting"
        );
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    };

    let Ok(user_uuid) = user_uuid.parse::<uuid::Uuid>() else {
        tracing::warn!(
            session = %session.id,
            user_uuid = %user_uuid,
            "stripe checkout metadata user_uuid is not a uuid, not crediting"
        );
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    };
    let Ok(credits) = credits.parse::<i64>() else {
        tracing::warn!(
            session = %session.id,
            credits = %credits,
            "stripe checkout metadata credits is not an integer, not crediting"
        );
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    };
    if credits <= 0 {
        tracing::warn!(
            session = %session.id,
            credits,
            "stripe checkout credits must be positive, not crediting"
        );
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    }

    // Idempotency: a retried Stripe event must never credit twice.
    if UserCreditTransaction::exists_by_reference(&state.database, &session.id).await? {
        tracing::info!(
            session = %session.id,
            "stripe checkout already credited, ignoring replay"
        );
        return ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok();
    }

    let (credit, _) = UserCredit::apply(
        &state,
        user_uuid,
        credits,
        UserCreditTransactionKind::Purchase,
        None,
        Some(session.id.clone().into()),
        Some(format!("Stripe checkout {session_id}", session_id = session.id).into()),
    )
    .await?;

    tracing::info!(
        user = %user_uuid,
        session = %session.id,
        credits,
        balance_credits = credit.balance_credits,
        "credited purchase from stripe webhook"
    );

    ApiResponse::new_serialized(serde_json::json!({ "received": true })).ok()
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .route("/", axum::routing::post(route))
        .with_state(state.clone())
}
