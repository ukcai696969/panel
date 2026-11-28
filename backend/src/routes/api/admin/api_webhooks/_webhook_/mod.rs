use super::State;
use axum::{
    extract::{Path, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use shared::{
    GetState,
    models::{
        ByUuid,
        user::GetPermissionManager,
        webhook_subscription::WebhookSubscription,
    },
    response::ApiResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};

pub type GetWebhookSubscription =
    shared::extract::ConsumingExtension<WebhookSubscription>;

pub async fn auth(
    state: GetState,
    permissions: GetPermissionManager,
    Path(webhook): Path<Vec<String>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let webhook_uuid = match webhook.first().map(|s| s.parse::<uuid::Uuid>()) {
        Some(Ok(id)) => id,
        _ => {
            return Ok(ApiResponse::error("invalid webhook uuid")
                .with_status(StatusCode::BAD_REQUEST)
                .into_response());
        }
    };

    if let Err(err) = permissions.has_admin_permission("webhooks.read") {
        return Ok(err.into_response());
    }

    let webhook =
        WebhookSubscription::by_uuid_optional(&state.database, webhook_uuid).await;
    let webhook = match webhook {
        Ok(Some(webhook)) => webhook,
        Ok(None) => {
            return Ok(ApiResponse::error("webhook not found")
                .with_status(StatusCode::NOT_FOUND)
                .into_response());
        }
        Err(err) => return Ok(ApiResponse::from(err).into_response()),
    };

    req.extensions_mut().insert(webhook);

    Ok(next.run(req).await)
}

mod get {
    use crate::routes::api::admin::api_webhooks::_webhook_::GetWebhookSubscription;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{IntoAdminApiObject, user::GetPermissionManager},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        webhook: shared::models::webhook_subscription::AdminApiWebhookSubscription,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook subscription ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetWebhookSubscription,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.read")?;

        ApiResponse::new_serialized(Response {
            webhook: webhook.0.into_admin_api_object(&state, ()).await?,
        })
        .ok()
    }
}

mod patch {
    use crate::routes::api::admin::api_webhooks::_webhook_::GetWebhookSubscription;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            UpdatableModel, admin_activity::GetAdminActivityLogger,
            user::GetPermissionManager,
            webhook_subscription::UpdateWebhookSubscriptionOptions,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(patch, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
        (status = BAD_REQUEST, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook subscription ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(UpdateWebhookSubscriptionOptions))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        mut webhook: GetWebhookSubscription,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<UpdateWebhookSubscriptionOptions>,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.update")?;

        webhook.update(&state, data).await?;

        activity_logger
            .log(
                "webhook:update",
                serde_json::json!({
                    "uuid": webhook.uuid,
                    "name": webhook.name,
                    "url": webhook.url,
                    "events": webhook.events,
                    "enabled": webhook.enabled,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod delete {
    use crate::routes::api::admin::api_webhooks::_webhook_::GetWebhookSubscription;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            DeletableModel, admin_activity::GetAdminActivityLogger,
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(delete, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook subscription ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetWebhookSubscription,
        activity_logger: GetAdminActivityLogger,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.delete")?;

        activity_logger
            .log(
                "webhook:delete",
                serde_json::json!({
                    "uuid": webhook.uuid,
                    "name": webhook.name,
                    "url": webhook.url,
                }),
            )
            .await;

        webhook.delete(&state, ()).await?;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod test {
    use crate::routes::api::admin::api_webhooks::_webhook_::GetWebhookSubscription;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::user::GetPermissionManager,
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        success: bool,
        status_code: Option<i32>,
        error: Option<String>,
        attempts: i32,
    }

    #[utoipa::path(post, path = "/test", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook subscription ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetWebhookSubscription,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.update")?;

        let outcome = shared::webhooks::send_test_event(&state, &webhook.0).await?;

        ApiResponse::new_serialized(Response {
            success: outcome.success,
            status_code: outcome.status_code,
            error: outcome.error,
            attempts: outcome.attempts,
        })
        .ok()
    }
}

mod rotate_secret {
    use crate::routes::api::admin::api_webhooks::_webhook_::GetWebhookSubscription;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        crypt::EncryptedString,
        models::{
            admin_activity::GetAdminActivityLogger, user::GetPermissionManager,
            webhook_subscription::WebhookSubscription,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        /// The new plaintext signing secret. Shown exactly once.
        secret: String,
    }

    #[utoipa::path(post, path = "/rotate-secret", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook subscription ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        mut webhook: GetWebhookSubscription,
        activity_logger: GetAdminActivityLogger,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.update")?;

        let secret = WebhookSubscription::generate_secret();
        let encrypted = EncryptedString::from_plaintext(&secret, &state.database).await?;

        webhook.rotate_secret(&state, encrypted).await?;

        activity_logger
            .log(
                "webhook:rotate-secret",
                serde_json::json!({
                    "uuid": webhook.uuid,
                    "name": webhook.name,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response { secret }).ok()
    }
}

mod deliveries {
    use crate::routes::api::admin::api_webhooks::_webhook_::GetWebhookSubscription;
    use axum::extract::Query;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoAdminApiObject, Pagination, PaginationParams,
            user::GetPermissionManager,
            webhook_delivery::WebhookDelivery,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        deliveries: Pagination<shared::models::webhook_delivery::AdminApiWebhookDelivery>,
    }

    #[utoipa::path(get, path = "/deliveries", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook subscription ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "page" = i64, Query,
            description = "The page number",
            example = "1",
        ),
        (
            "per_page" = i64, Query,
            description = "The number of items per page",
            example = "10",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetWebhookSubscription,
        Query(params): Query<PaginationParams>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(axum::http::StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("webhooks.read")?;

        let deliveries = WebhookDelivery::by_subscription_uuid_with_pagination(
            &state.database,
            webhook.uuid,
            params.page,
            params.per_page,
        )
        .await?;

        ApiResponse::new_serialized(Response {
            deliveries: deliveries
                .try_async_map(|delivery| delivery.into_admin_api_object(&state, ()))
                .await?,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(patch::route))
        .routes(routes!(delete::route))
        .routes(routes!(test::route))
        .routes(routes!(rotate_secret::route))
        .routes(routes!(deliveries::route))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state.clone())
}
