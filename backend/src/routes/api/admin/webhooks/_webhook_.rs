use super::State;
use axum::{
    extract::{Path, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use shared::{
    GetState,
    models::{server_webhook::ServerWebhook, user::GetPermissionManager},
    response::ApiResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};

pub type GetGlobalWebhook = shared::extract::ConsumingExtension<ServerWebhook>;

pub async fn auth(
    state: GetState,
    permissions: GetPermissionManager,
    Path(webhook): Path<Vec<String>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let webhook = match webhook.get(1).map(|s| s.parse::<uuid::Uuid>()) {
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

    let webhook = ServerWebhook::by_uuid_global(&state.database, webhook).await;
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
    use crate::routes::api::admin::webhooks::_webhook_::GetGlobalWebhook;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{IntoAdminApiObject, user::GetPermissionManager},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    pub struct Response {
        webhook: shared::models::server_webhook::AdminApiServerWebhook,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetGlobalWebhook,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.read")?;

        ApiResponse::new_serialized(Response {
            webhook: webhook.0.into_admin_api_object(&state, ()).await?,
        })
        .ok()
    }
}

mod patch {
    use crate::routes::api::admin::webhooks::_webhook_::GetGlobalWebhook;
    use axum::http::StatusCode;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            UpdatableModel, server_webhook::UpdateServerWebhookOptions, user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(patch, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = BAD_REQUEST, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(UpdateServerWebhookOptions))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        mut webhook: GetGlobalWebhook,
        shared::Payload(mut data): shared::Payload<UpdateServerWebhookOptions>,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.update")?;

        if let Some(url) = &data.url {
            match shared::webhooks::validate_webhook_url(&state.env, url) {
                Ok(validated) => {
                    if webhook.kind == shared::models::server_webhook::ServerWebhookKind::Discord
                        && let Err(err) = shared::webhooks::validate_discord_url(&validated)
                    {
                        return ApiResponse::error(err)
                            .with_status(StatusCode::BAD_REQUEST)
                            .ok();
                    }
                    data.url = Some(validated.to_string().into());
                }
                Err(err) => {
                    return ApiResponse::error(err)
                        .with_status(StatusCode::BAD_REQUEST)
                        .ok();
                }
            }
        }

        webhook.update(&state, data).await?;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod delete {
    use crate::routes::api::admin::webhooks::_webhook_::GetGlobalWebhook;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{DeletableModel, user::GetPermissionManager},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(delete, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "webhook" = uuid::Uuid,
            description = "The webhook ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetGlobalWebhook,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.delete")?;

        webhook.delete(&state, ()).await?;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(patch::route))
        .routes(routes!(delete::route))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state.clone())
}
