// Admin billing routes: pricing, balances, transactions, adjustments.
//
// Mounted at `/api/admin/billing` (see `backend/src/routes/api/admin/mod.rs`).
// All routes require an admin permission under `billing.*`. Those
// permission strings are new: they must also be added to the admin
// permission list the frontend offers (see MANIFEST.md), otherwise only
// unrestricted admins (`user.admin = true`) can use them.
//
// Pricing is stored in settings (`AppSettingsBilling`); Stripe
// credentials are stored there too (secret key + webhook secret
// encrypted at rest), with environment variables as overrides — see
// `crate::billing::StripeConfig::resolve`. The admin Stripe endpoints
// (`GET`/`PUT /stripe` below) never return a full secret, only masked
// values and configured flags.

use super::State;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Masked view of the Stripe settings for the admin API. This is the
/// only shape in which Stripe credentials leave the backend towards an
/// admin: the secrets appear only as `sk_test_…1234`-style masked
/// values (or `null` when nothing is stored), plus configured and
/// env-override flags. The publishable key is public by design and is
/// returned in full; the checkout URL template is not secret either.
#[derive(ToSchema, Serialize)]
struct StripeSettingsResponse {
    stripe_secret_key_masked: Option<String>,
    stripe_secret_key_configured: bool,
    stripe_secret_key_from_env: bool,
    stripe_webhook_secret_masked: Option<String>,
    stripe_webhook_secret_configured: bool,
    stripe_webhook_secret_from_env: bool,
    stripe_publishable_key: Option<String>,
    stripe_publishable_key_from_env: bool,
    checkout_url_template: String,
    checkout_url_template_from_env: bool,
    /// A secret key resolves from the environment or settings, so
    /// Stripe can be effectively configured even when nothing is stored.
    stripe_configured: bool,
}

/// A non-empty env var, or `None` when unset/empty. An empty env var
/// counts as unset everywhere in the Stripe precedence rules.
fn stripe_env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn stripe_settings_response(
    billing: &shared::settings::billing::AppSettingsBilling,
) -> StripeSettingsResponse {
    let secret_from_env = stripe_env_var("STRIPE_SECRET_KEY").is_some();
    let webhook_from_env = stripe_env_var("STRIPE_WEBHOOK_SECRET").is_some();
    let publishable_from_env = stripe_env_var("STRIPE_PUBLISHABLE_KEY").is_some();
    let template_from_env = stripe_env_var("BILLING_CHECKOUT_URL_TEMPLATE").is_some();

    StripeSettingsResponse {
        stripe_secret_key_masked: billing.stripe_secret_key_masked(),
        stripe_secret_key_configured: billing.stripe_secret_key.is_some(),
        stripe_secret_key_from_env: secret_from_env,
        stripe_webhook_secret_masked: billing.stripe_webhook_secret_masked(),
        stripe_webhook_secret_configured: billing.stripe_webhook_secret.is_some(),
        stripe_webhook_secret_from_env: webhook_from_env,
        stripe_publishable_key: billing
            .stripe_publishable_key
            .as_ref()
            .map(|key| key.to_string()),
        stripe_publishable_key_from_env: publishable_from_env,
        checkout_url_template: billing.checkout_url_template.to_string(),
        checkout_url_template_from_env: template_from_env,
        stripe_configured: secret_from_env || billing.stripe_secret_key.is_some(),
    }
}

mod get_pricing {
    use serde::Serialize;
    use shared::{
        GetState,
        models::user::GetPermissionManager,
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    /// Pricing only — the Stripe credentials in `AppSettingsBilling`
    /// are deliberately NOT part of this response (they have their own
    /// masked endpoint, `GET /stripe`, below). Keeping them out here
    /// also means the existing frontend pricing helpers keep working
    /// unchanged.
    #[derive(ToSchema, Serialize)]
    struct Pricing {
        enabled: bool,
        currency: compact_str::CompactString,
        price_per_gb_ram_per_hour_credits: i64,
        price_per_100_cpu_per_hour_credits: i64,
        price_per_gb_disk_per_hour_credits: i64,
        suspend_on_negative_balance: bool,
        minimum_purchase_credits: i64,
        maximum_purchase_credits: i64,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        billing: Pricing,
    }

    #[utoipa::path(get, path = "/pricing", responses(
        (status = OK, body = inline(Response)),
    ))]
    pub async fn route(state: GetState, permissions: GetPermissionManager) -> ApiResponseResult {
        permissions.has_admin_permission("billing.read")?;

        let billing = state.settings.get_as(|s| s.billing.clone()).await?;

        ApiResponse::new_serialized(Response {
            billing: Pricing {
                enabled: billing.enabled,
                currency: billing.currency,
                price_per_gb_ram_per_hour_credits: billing.price_per_gb_ram_per_hour_credits,
                price_per_100_cpu_per_hour_credits: billing.price_per_100_cpu_per_hour_credits,
                price_per_gb_disk_per_hour_credits: billing.price_per_gb_disk_per_hour_credits,
                suspend_on_negative_balance: billing.suspend_on_negative_balance,
                minimum_purchase_credits: billing.minimum_purchase_credits,
                maximum_purchase_credits: billing.maximum_purchase_credits,
            },
        })
        .ok()
    }
}

mod put_pricing {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        censor::Censor,
        models::{admin_activity::GetAdminActivityLogger, user::GetPermissionManager},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        #[garde(skip)]
        pub enabled: Option<bool>,
        #[garde(length(chars, min = 3, max = 3))]
        #[schema(min_length = 3, max_length = 3)]
        pub currency: Option<compact_str::CompactString>,
        #[garde(range(min = 0, max = 1_000_000))]
        #[schema(minimum = 0, maximum = 1_000_000)]
        pub price_per_gb_ram_per_hour_credits: Option<i64>,
        #[garde(range(min = 0, max = 1_000_000))]
        #[schema(minimum = 0, maximum = 1_000_000)]
        pub price_per_100_cpu_per_hour_credits: Option<i64>,
        #[garde(range(min = 0, max = 1_000_000))]
        #[schema(minimum = 0, maximum = 1_000_000)]
        pub price_per_gb_disk_per_hour_credits: Option<i64>,
        #[garde(skip)]
        pub suspend_on_negative_balance: Option<bool>,
        #[garde(range(min = 1, max = 1_000_000))]
        #[schema(minimum = 1, maximum = 1_000_000)]
        pub minimum_purchase_credits: Option<i64>,
        #[garde(range(min = 1, max = 10_000_000))]
        #[schema(minimum = 1, maximum = 10_000_000)]
        pub maximum_purchase_credits: Option<i64>,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(put, path = "/pricing", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("billing.update")?;

        if let (Some(min), Some(max)) = (data.minimum_purchase_credits, data.maximum_purchase_credits)
            && min > max
        {
            return ApiResponse::error("minimum purchase must not exceed maximum purchase")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        let mut settings = state.settings.get_mut().await?;
        if let Some(enabled) = data.enabled {
            settings.billing.enabled = enabled;
        }
        if let Some(currency) = data.currency {
            settings.billing.currency = currency.to_ascii_uppercase().into();
        }
        if let Some(price) = data.price_per_gb_ram_per_hour_credits {
            settings.billing.price_per_gb_ram_per_hour_credits = price;
        }
        if let Some(price) = data.price_per_100_cpu_per_hour_credits {
            settings.billing.price_per_100_cpu_per_hour_credits = price;
        }
        if let Some(price) = data.price_per_gb_disk_per_hour_credits {
            settings.billing.price_per_gb_disk_per_hour_credits = price;
        }
        if let Some(suspend) = data.suspend_on_negative_balance {
            settings.billing.suspend_on_negative_balance = suspend;
        }
        if let Some(minimum) = data.minimum_purchase_credits {
            settings.billing.minimum_purchase_credits = minimum;
        }
        if let Some(maximum) = data.maximum_purchase_credits {
            settings.billing.maximum_purchase_credits = maximum;
        }

        // Censored: `AppSettingsBilling` now carries Stripe secrets,
        // which must never land in the activity log.
        let billing_json = serde_json::to_value(settings.billing.clone().censored())
            .unwrap_or(serde_json::Value::Null);
        settings.save().await?;

        activity_logger
            .log("billing:pricing.update", billing_json)
            .await;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod get_stripe {
    use shared::{
        GetState,
        models::user::GetPermissionManager,
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[utoipa::path(get, path = "/stripe", responses(
        (status = OK, body = super::StripeSettingsResponse),
    ))]
    pub async fn route(state: GetState, permissions: GetPermissionManager) -> ApiResponseResult {
        permissions.has_admin_permission("billing.read")?;

        let billing = state.settings.get_as(|s| s.billing.clone()).await?;

        ApiResponse::new_serialized(super::stripe_settings_response(&billing)).ok()
    }
}

mod put_stripe {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::Deserialize;
    use shared::{
        ApiError, GetState,
        models::{admin_activity::GetAdminActivityLogger, user::GetPermissionManager},
        response::{ApiResponse, ApiResponseResult},
        settings::billing::AppSettingsBilling,
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        /// New secret key (`sk_test_…`). `None` = leave unchanged;
        /// `""` = clear the stored key; the masked value returned by
        /// `GET /stripe` = leave unchanged. Live keys are refused.
        #[garde(skip)]
        pub stripe_secret_key: Option<String>,
        /// New webhook secret (`whsec_…`). Same `None` / `""` /
        /// masked-value rules as the secret key.
        #[garde(skip)]
        pub stripe_webhook_secret: Option<String>,
        /// New publishable key (`pk_test_…`). `None` = leave unchanged;
        /// `""` = clear. Returned in full by `GET /stripe`: it is
        /// public by design.
        #[garde(skip)]
        pub stripe_publishable_key: Option<String>,
        /// New checkout URL template. `None` = leave unchanged; `""` =
        /// reset to the default template. Must contain `{session_id}`.
        #[garde(skip)]
        pub checkout_url_template: Option<String>,
    }

    #[utoipa::path(put, path = "/stripe", responses(
        (status = OK, body = super::StripeSettingsResponse),
        (status = BAD_REQUEST, body = ApiError),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("billing.update")?;

        // Validate everything before mutating anything, so a bad field
        // cannot leave a half-applied update behind.
        if let Some(secret_key) = data.stripe_secret_key.as_deref()
            && !secret_key.is_empty()
            && !AppSettingsBilling::is_masked_secret(secret_key)
            && !secret_key.starts_with("sk_test_")
        {
            return ApiResponse::error(
                "stripe secret key must start with sk_test_ — live keys are refused (test mode only)",
            )
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
        }
        if let Some(publishable_key) = data.stripe_publishable_key.as_deref()
            && !publishable_key.is_empty()
            && !publishable_key.starts_with("pk_test_")
        {
            return ApiResponse::error(
                "stripe publishable key must start with pk_test_ — live keys are refused (test mode only)",
            )
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
        }
        if let Some(template) = data.checkout_url_template.as_deref()
            && !template.is_empty()
            && !template.contains("{session_id}")
        {
            return ApiResponse::error("checkout URL template must contain {session_id}")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }
        if let Some(template) = data.checkout_url_template.as_deref()
            && template.chars().count() > 1024
        {
            return ApiResponse::error("checkout URL template is too long")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        let mut settings = state.settings.get_mut().await?;

        // Per-field outcome, for the activity log. No secret value —
        // not even a masked one — is ever logged.
        let mut secret_changed = false;
        let mut webhook_changed = false;
        let mut publishable_changed = false;
        let mut template_changed = false;

        if let Some(secret_key) = data.stripe_secret_key.as_deref() {
            if secret_key.is_empty() {
                if settings.billing.stripe_secret_key.take().is_some() {
                    secret_changed = true;
                }
            } else if !AppSettingsBilling::is_masked_secret(secret_key) {
                settings.billing.stripe_secret_key = Some(secret_key.into());
                secret_changed = true;
            }
            // A masked value is the UI sending back what it was shown:
            // leave the stored secret untouched.
        }
        if let Some(webhook_secret) = data.stripe_webhook_secret.as_deref() {
            if webhook_secret.is_empty() {
                if settings.billing.stripe_webhook_secret.take().is_some() {
                    webhook_changed = true;
                }
            } else if !AppSettingsBilling::is_masked_secret(webhook_secret) {
                settings.billing.stripe_webhook_secret = Some(webhook_secret.into());
                webhook_changed = true;
            }
        }
        if let Some(publishable_key) = data.stripe_publishable_key.as_deref() {
            if publishable_key.is_empty() {
                if settings.billing.stripe_publishable_key.take().is_some() {
                    publishable_changed = true;
                }
            } else {
                settings.billing.stripe_publishable_key = Some(publishable_key.into());
                publishable_changed = true;
            }
        }
        if let Some(template) = data.checkout_url_template.as_deref() {
            if template.is_empty() {
                if settings.billing.checkout_url_template.as_str()
                    != AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE
                {
                    settings.billing.checkout_url_template =
                        AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE.into();
                    template_changed = true;
                }
            } else if settings.billing.checkout_url_template.as_str() != template {
                settings.billing.checkout_url_template = template.into();
                template_changed = true;
            }
        }

        let response = super::stripe_settings_response(&settings.billing);
        settings.save().await?;

        activity_logger
            .log(
                "billing:stripe.update",
                serde_json::json!({
                    "stripe_secret_key_changed": secret_changed,
                    "stripe_webhook_secret_changed": webhook_changed,
                    "stripe_publishable_key_changed": publishable_changed,
                    "checkout_url_template_changed": template_changed,
                }),
            )
            .await;

        ApiResponse::new_serialized(response).ok()
    }
}

mod get_balances {
    use axum::{extract::Query, http::StatusCode};
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoAdminApiObject, Pagination, PaginationParamsWithSearch,
            user::GetPermissionManager, user_credit::UserCredit,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        balances: Pagination<shared::models::user_credit::AdminApiUserCredit>,
    }

    #[utoipa::path(get, path = "/balances", responses(
        (status = OK, body = inline(Response)),
    ), params(
        ("page" = i64, Query, description = "The page number", example = "1"),
        ("per_page" = i64, Query, description = "The number of items per page", example = "10"),
        ("search" = Option<String>, Query, description = "Search by username, email or user uuid"),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        Query(params): Query<PaginationParamsWithSearch>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("billing.read")?;

        let balances = UserCredit::all_with_pagination(
            &state.database,
            params.page,
            params.per_page,
            params.search.as_deref(),
        )
        .await?;

        ApiResponse::new_serialized(Response {
            balances: balances
                .try_async_map(|balance| balance.into_admin_api_object(&state, ()))
                .await?,
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
            IntoAdminApiObject, Pagination, PaginationParamsWithSearch,
            user::GetPermissionManager,
            user_credit_transaction::UserCreditTransaction,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        transactions: Pagination<shared::models::user_credit_transaction::AdminApiUserCreditTransaction>,
    }

    #[utoipa::path(get, path = "/transactions", responses(
        (status = OK, body = inline(Response)),
    ), params(
        ("page" = i64, Query, description = "The page number", example = "1"),
        ("per_page" = i64, Query, description = "The number of items per page", example = "10"),
        ("search" = Option<String>, Query, description = "Search by reference, description or uuid"),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        Query(params): Query<PaginationParamsWithSearch>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("billing.read")?;

        let transactions = UserCreditTransaction::all_with_pagination(
            &state.database,
            params.page,
            params.per_page,
            params.search.as_deref(),
        )
        .await?;

        ApiResponse::new_serialized(Response {
            transactions: transactions
                .try_async_map(|transaction| transaction.into_admin_api_object(&state, ()))
                .await?,
        })
        .ok()
    }
}

mod user {
    use super::State;
    use utoipa_axum::{router::OpenApiRouter, routes};

    mod get {
        use axum::{extract::Path, http::StatusCode};
        use serde::Serialize;
        use shared::{
            GetState,
            models::{
                ByUuid,
                user::{GetPermissionManager, User},
                user_credit::UserCredit,
            },
            response::{ApiResponse, ApiResponseResult},
        };
        use utoipa::ToSchema;

        #[derive(ToSchema, Serialize)]
        struct Response {
            pub user_uuid: uuid::Uuid,
            pub balance_credits: i64,
            pub updated: Option<chrono::DateTime<chrono::Utc>>,
        }

        #[utoipa::path(get, path = "/", responses(
            (status = OK, body = inline(Response)),
        ), params(
            ("user" = String, Path, description = "The user uuid"),
        ))]
        pub async fn route(
            state: GetState,
            permissions: GetPermissionManager,
            Path(user_uuid): Path<uuid::Uuid>,
        ) -> ApiResponseResult {
            permissions.has_admin_permission("billing.read")?;

            let user = User::by_uuid_optional(&state.database, user_uuid).await?;
            let Some(user) = user else {
                return ApiResponse::error("user not found")
                    .with_status(StatusCode::NOT_FOUND)
                    .ok();
            };

            // A user with no row yet has a zero balance; do not create a
            // row on a read.
            let credit = UserCredit::by_user_uuid(&state.database, user.uuid).await?;

            ApiResponse::new_serialized(Response {
                user_uuid: user.uuid,
                balance_credits: credit.as_ref().map_or(0, |c| c.balance_credits),
                updated: credit.map(|c| c.updated.and_utc()),
            })
            .ok()
        }
    }

    mod adjust {
        use axum::{extract::Path, http::StatusCode};
        use garde::Validate;
        use serde::{Deserialize, Serialize};
        use shared::{
            ApiError, GetState,
            models::{
                ByUuid, IntoAdminApiObject,
                admin_activity::GetAdminActivityLogger,
                user::{GetPermissionManager, User},
                user_credit::UserCredit,
                user_credit_transaction::UserCreditTransactionKind,
            },
            response::{ApiResponse, ApiResponseResult},
        };
        use utoipa::ToSchema;

        #[derive(ToSchema, Validate, Deserialize)]
        pub struct Payload {
            /// Signed credits to add (positive) or remove (negative).
            /// Zero is rejected; this is a manual grant / correction,
            /// not a purchase.
            #[garde(range(min = -1_000_000, max = 1_000_000))]
            #[schema(minimum = -1_000_000, maximum = 1_000_000)]
            pub amount_credits: i64,
            #[garde(length(chars, min = 1, max = 512))]
            #[schema(min_length = 1, max_length = 512)]
            pub description: Option<compact_str::CompactString>,
        }

        #[derive(ToSchema, Serialize)]
        struct Response {
            #[schema(inline)]
            balance: shared::models::user_credit::AdminApiUserCredit,
        }

        #[utoipa::path(post, path = "/adjust", responses(
            (status = OK, body = inline(Response)),
            (status = BAD_REQUEST, body = ApiError),
        ), params(
            ("user" = String, Path, description = "The user uuid"),
        ), request_body = inline(Payload))]
        pub async fn route(
            state: GetState,
            permissions: GetPermissionManager,
            activity_logger: GetAdminActivityLogger,
            Path(user_uuid): Path<uuid::Uuid>,
            shared::Payload(data): shared::Payload<Payload>,
        ) -> ApiResponseResult {
            if let Err(errors) = shared::utils::validate_data(&data) {
                return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                    .with_status(StatusCode::BAD_REQUEST)
                    .ok();
            }

            permissions.has_admin_permission("billing.adjust")?;

            if data.amount_credits == 0 {
                return ApiResponse::error("amount must not be zero")
                    .with_status(StatusCode::BAD_REQUEST)
                    .ok();
            }

            let user = User::by_uuid_optional(&state.database, user_uuid).await?;
            let Some(user) = user else {
                return ApiResponse::error("user not found")
                    .with_status(StatusCode::NOT_FOUND)
                    .ok();
            };

            let (credit, transaction) = UserCredit::apply(
                &state,
                user.uuid,
                data.amount_credits,
                UserCreditTransactionKind::Adjustment,
                None,
                None,
                data.description.clone(),
            )
            .await?;

            activity_logger
                .log(
                    "billing:adjust",
                    serde_json::json!({
                        "user": user.uuid,
                        "amount_credits": data.amount_credits,
                        "balance_after_credits": credit.balance_credits,
                        "transaction": transaction.uuid,
                        "description": data.description,
                    }),
                )
                .await;

            ApiResponse::new_serialized(Response {
                balance: credit.into_admin_api_object(&state, ()).await?,
            })
            .ok()
        }
    }

    pub fn router(state: &State) -> OpenApiRouter<State> {
        OpenApiRouter::new()
            .routes(routes!(get::route))
            .routes(routes!(adjust::route))
            .with_state(state.clone())
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get_pricing::route))
        .routes(routes!(put_pricing::route))
        .routes(routes!(get_stripe::route))
        .routes(routes!(put_stripe::route))
        .routes(routes!(get_balances::route))
        .routes(routes!(get_transactions::route))
        .nest("/users/{user}", user::router(state))
        .with_state(state.clone())
}
