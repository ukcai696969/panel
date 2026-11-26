// Client billing routes: balance, history, Stripe checkout.
//
// Mounted at `/api/client/billing` (see `backend/src/routes/api/client/mod.rs`).
// These run behind the normal client auth middleware, so `GetUser` is
// the authenticated user (or the impersonated user, via the client auth
// flow — an admin impersonating a user sees that user's balance, which
// is the same behaviour as the rest of the client API).
//
// The Stripe webhook is NOT here: it has no session auth and lives in
// `backend/src/routes/api/billing_webhook.rs`, mounted at
// `/api/billing-webhook`.

use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use serde::Serialize;
    use shared::{
        GetState,
        models::{user::GetUser, user_credit::UserCredit},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Pricing {
        enabled: bool,
        currency: compact_str::CompactString,
        price_per_gb_ram_per_hour_credits: i64,
        price_per_100_cpu_per_hour_credits: i64,
        price_per_gb_disk_per_hour_credits: i64,
        minimum_purchase_credits: i64,
        maximum_purchase_credits: i64,
        /// The Stripe publishable key is public by design; the secret
        /// key is never sent to the browser.
        stripe_publishable_key: Option<String>,
        stripe_configured: bool,
    }

    #[derive(ToSchema, Serialize)]
    struct Balance {
        balance_credits: i64,
        updated: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        balance: Balance,
        #[schema(inline)]
        pricing: Pricing,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
    ))]
    pub async fn route(state: GetState, user: GetUser) -> ApiResponseResult {
        let billing = state.settings.get_as(|s| s.billing.clone()).await?;
        // Resolved from settings with the environment as an override.
        // Only the publishable key (public by design) and the
        // configured flag reach the browser — never the secret key.
        let stripe = crate::billing::StripeConfig::resolve(&state.settings).await?;

        // A zero balance with no row yet reads as 0; do not create a
        // row on a read.
        let credit = UserCredit::by_user_uuid(&state.database, user.uuid).await?;
        let balance = Balance {
            balance_credits: credit.as_ref().map_or(0, |c| c.balance_credits),
            updated: credit.map(|c| c.updated.and_utc()),
        };

        ApiResponse::new_serialized(Response {
            balance,
            pricing: Pricing {
                enabled: billing.enabled,
                currency: billing.currency,
                price_per_gb_ram_per_hour_credits: billing.price_per_gb_ram_per_hour_credits,
                price_per_100_cpu_per_hour_credits: billing
                    .price_per_100_cpu_per_hour_credits,
                price_per_gb_disk_per_hour_credits: billing.price_per_gb_disk_per_hour_credits,
                minimum_purchase_credits: billing.minimum_purchase_credits,
                maximum_purchase_credits: billing.maximum_purchase_credits,
                stripe_publishable_key: stripe
                    .as_ref()
                    .and_then(|config| config.publishable_key.clone()),
                stripe_configured: stripe.is_some(),
            },
        })
        .ok()
    }
}

mod get_transactions {
    use axum::{extract::Query, http::StatusCode};
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoApiObject, Pagination, PaginationParams,
            user::GetUser,
            user_credit_transaction::UserCreditTransaction,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        transactions: Pagination<shared::models::user_credit_transaction::ApiUserCreditTransaction>,
    }

    #[utoipa::path(get, path = "/transactions", responses(
        (status = OK, body = inline(Response)),
    ), params(
        ("page" = i64, Query, description = "The page number", example = "1"),
        ("per_page" = i64, Query, description = "The number of items per page", example = "10"),
    ))]
    pub async fn route(
        state: GetState,
        user: GetUser,
        Query(params): Query<PaginationParams>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        let transactions = UserCreditTransaction::by_user_uuid_with_pagination(
            &state.database,
            user.uuid,
            params.page,
            params.per_page,
        )
        .await?;

        ApiResponse::new_serialized(Response {
            transactions: transactions
                .try_async_map(|transaction| transaction.into_api_object(&state, ()))
                .await?,
        })
        .ok()
    }
}

mod post_checkout {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::user::GetUser,
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        /// Credits to buy. 1 credit = 1 minor currency unit, so this is
        /// also the Stripe amount in minor units.
        #[garde(range(min = 1, max = 10_000_000))]
        #[schema(minimum = 1, maximum = 10_000_000)]
        pub credits: i64,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        session_id: String,
        checkout_url: String,
        credits: i64,
    }

    #[utoipa::path(post, path = "/checkout", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        user: GetUser,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        let billing = state.settings.get_as(|s| s.billing.clone()).await?;
        if !billing.enabled {
            return ApiResponse::error("billing is disabled")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }
        if data.credits < billing.minimum_purchase_credits
            || data.credits > billing.maximum_purchase_credits
        {
            return ApiResponse::error(format!(
                "purchase must be between {} and {} credits",
                billing.minimum_purchase_credits, billing.maximum_purchase_credits
            ))
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
        }

        let Some(stripe) = crate::billing::StripeConfig::resolve(&state.settings).await?
        else {
            return ApiResponse::error("stripe is not configured")
                .with_status(StatusCode::SERVICE_UNAVAILABLE)
                .ok();
        };
        if let Err(err) = stripe.require_test_mode() {
            return ApiResponse::error(err.to_string())
                .with_status(StatusCode::SERVICE_UNAVAILABLE)
                .ok();
        }

        let app_url = state.settings.get_as(|s| s.app.url.clone()).await?;
        // STUB: no Stripe API call is made here. When the real
        // integration lands, replace this call with a Checkout Session
        // creation that sets metadata { user_uuid, credits } on the
        // session, so the webhook can credit the right user. The
        // session id returned here is the idempotency reference the
        // webhook uses.
        let session = crate::billing::create_checkout_session_stub(
            &app_url,
            data.credits,
            &billing.currency,
            &stripe.checkout_url_template,
        )?;

        tracing::info!(
            user = %user.uuid,
            session = %session.session_id,
            credits = data.credits,
            "created billing checkout session (stub, no charge made)"
        );

        ApiResponse::new_serialized(Response {
            session_id: session.session_id,
            checkout_url: session.checkout_url,
            credits: data.credits,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(get_transactions::route))
        .routes(routes!(post_checkout::route))
        .with_state(state.clone())
}
