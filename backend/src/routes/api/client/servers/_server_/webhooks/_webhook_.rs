use super::State;
use axum::{
    extract::{Path, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use shared::{
    GetState,
    models::{server::GetServer, server_webhook::ServerWebhook, user::GetPermissionManager},
    response::ApiResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};

pub type GetServerWebhook = shared::extract::ConsumingExtension<ServerWebhook>;

pub async fn auth(
    state: GetState,
    permissions: GetPermissionManager,
    server: GetServer,
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

    if let Err(err) = permissions.has_server_permission("webhooks.read") {
        return Ok(err.into_response());
    }

    let webhook = ServerWebhook::by_server_uuid_uuid(&state.database, server.uuid, webhook).await;
    let webhook = match webhook {
        Ok(Some(webhook)) => webhook,
        Ok(None) => {
            return Ok(ApiResponse::error("webhook not found")
                .with_status(StatusCode::NOT_FOUND)
                .into_response());
        }
        Err(err) => return Ok(ApiResponse::from(err).into_response()),
    };

    req.extensions_mut().insert(server.0);
    req.extensions_mut().insert(webhook);

    Ok(next.run(req).await)
}

mod get {
    use crate::routes::api::client::servers::_server_::webhooks::_webhook_::GetServerWebhook;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{IntoApiObject, user::GetPermissionManager},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    pub struct Response {
        webhook: shared::models::server_webhook::ApiServerWebhook,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "webhook" = uuid::Uuid,
            description = "The webhook ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        webhook: GetServerWebhook,
    ) -> ApiResponseResult {
        permissions.has_server_permission("webhooks.read")?;

        ApiResponse::new_serialized(Response {
            webhook: webhook.0.into_api_object(&state, ()).await?,
        })
        .ok()
    }
}

mod patch {
    use crate::routes::api::client::servers::_server_::webhooks::_webhook_::GetServerWebhook;
    use axum::http::StatusCode;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            UpdatableModel,
            server::{GetServer, GetServerActivityLogger},
            server_webhook::UpdateServerWebhookOptions,
            user::GetPermissionManager,
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
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "webhook" = uuid::Uuid,
            description = "The webhook ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(UpdateServerWebhookOptions))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        server: GetServer,
        activity_logger: GetServerActivityLogger,
        mut webhook: GetServerWebhook,
        shared::Payload(mut data): shared::Payload<UpdateServerWebhookOptions>,
    ) -> ApiResponseResult {
        permissions.has_server_permission("webhooks.update")?;

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
                    // Store the normalized URL, as the create route does.
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

        activity_logger
            .log(
                "server:webhook.update",
                serde_json::json!({
                    "uuid": webhook.uuid,
                    "name": webhook.name,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod delete {
    use crate::routes::api::client::servers::_server_::webhooks::_webhook_::GetServerWebhook;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            DeletableModel,
            server::{GetServer, GetServerActivityLogger},
            user::GetPermissionManager,
        },
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
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "webhook" = uuid::Uuid,
            description = "The webhook ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        server: GetServer,
        activity_logger: GetServerActivityLogger,
        webhook: GetServerWebhook,
    ) -> ApiResponseResult {
        permissions.has_server_permission("webhooks.delete")?;

        webhook.delete(&state, ()).await?;

        activity_logger
            .log(
                "server:webhook.delete",
                serde_json::json!({
                    "uuid": webhook.uuid,
                    "name": webhook.name,
                }),
            )
            .await;

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
