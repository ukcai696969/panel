// Admin billing routes: pricing, balances, transactions, adjustments.
//
// Mounted at `/api/admin/billing` (see `backend/src/routes/api/admin/mod.rs`).
// All routes require an admin permission under `billing.*`. Those
// permission strings are new: they must also be added to the admin
// permission list the frontend offers (see MANIFEST.md), otherwise only
// unrestricted admins (`user.admin = true`) can use them.
//
// Pricing is stored in settings (`AppSettingsBilling`); Stripe secrets
// are never here — they are environment variables only.

use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get_pricing {
    use serde::Serialize;
    use shared::{
        GetState,
        models::user::GetPermissionManager,
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        billing: shared::settings::billing::AppSettingsBilling,
    }

    #[utoipa::path(get, path = "/pricing", responses(
        (status = OK, body = inline(Response)),
    ))]
    pub async fn route(state: GetState, permissions: GetPermissionManager) -> ApiResponseResult {
        permissions.has_admin_permission("billing.read")?;

        let billing = state.settings.get_as(|s| s.billing.clone()).await?;

        ApiResponse::new_serialized(Response { billing }).ok()
    }
}

mod put_pricing {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
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

        let billing_json = serde_json::to_value(&settings.billing)
            .unwrap_or(serde_json::Value::Null);
        settings.save().await?;

        activity_logger
            .log("billing:pricing.update", billing_json)
            .await;

        ApiResponse::new_serialized(Response {}).ok()
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
        .routes(routes!(get_balances::route))
        .routes(routes!(get_transactions::route))
        .nest("/users/{user}", user::router(state))
        .with_state(state.clone())
}
