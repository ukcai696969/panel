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
        server_template::ServerTemplate,
        user::{GetPermissionManager, GetUser},
    },
    response::ApiResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};

pub type GetServerTemplate = crate::extract::ConsumingExtension<ServerTemplate>;

mod instantiate;

pub async fn auth(
    state: GetState,
    permissions: GetPermissionManager,
    user: GetUser,
    Path(server_template): Path<Vec<String>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Err(err) = permissions.has_user_permission("servers.read") {
        return Ok(err.into_response());
    }

    let uuid = match uuid::Uuid::parse_str(&server_template[0]) {
        Ok(uuid) => uuid,
        Err(_) => {
            return Ok(ApiResponse::error("server template not found")
                .with_status(StatusCode::NOT_FOUND)
                .into_response());
        }
    };

    let is_admin = permissions
        .has_admin_permission("server-templates.read")
        .is_ok();
    let template = ServerTemplate::by_uuid_visible(&state.database, uuid, user.uuid, is_admin).await;
    let template = match template {
        Ok(Some(template)) => template,
        Ok(None) => {
            return Ok(ApiResponse::error("server template not found")
                .with_status(StatusCode::NOT_FOUND)
                .into_response());
        }
        Err(err) => return Ok(ApiResponse::from(err).into_response()),
    };

    req.extensions_mut().insert(template);

    Ok(next.run(req).await)
}

mod get {
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{IntoApiObject, server_template::ServerTemplate},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        server_template: shared::models::server_template::ApiServerTemplate,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = NOT_FOUND, body = ApiError),
    ), params(
        (
            "server_template" = uuid::Uuid,
            description = "The server template ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        template: super::GetServerTemplate,
    ) -> ApiResponseResult {
        ApiResponse::new_serialized(Response {
            server_template: template.0.into_api_object(&state, ()).await?,
        })
        .ok()
    }
}

mod delete {
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            DeletableModel,
            server_template::ServerTemplate,
            user::{GetPermissionManager, GetUser},
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
            "server_template" = uuid::Uuid,
            description = "The server template ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        user: GetUser,
        template: super::GetServerTemplate,
    ) -> ApiResponseResult {
        // Only the owner of a personal template (or an admin) may delete it;
        // global templates are admin-only and go through the admin API.
        if template.owner_uuid != Some(user.uuid)
            && permissions
                .has_admin_permission("server-templates.delete")
                .is_err()
        {
            return ApiResponse::error("only the template owner can delete it")
                .with_status(axum::http::StatusCode::FORBIDDEN)
                .ok();
        }

        template.0.delete(&state, ()).await?;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(delete::route))
        .nest("/instantiate", instantiate::router(state))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state.clone())
}
