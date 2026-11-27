use super::State;
use axum::{
    extract::{Path, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use shared::{
    GetState,
    models::{ByUuid, server_template::ServerTemplate, user::GetPermissionManager},
    response::ApiResponse,
};
use utoipa_axum::{router::OpenApiRouter, routes};

pub type GetServerTemplate = shared::extract::ConsumingExtension<ServerTemplate>;

mod instantiate;

pub async fn auth(
    state: GetState,
    permissions: GetPermissionManager,
    Path(server_template): Path<Vec<String>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Err(err) = permissions.has_admin_permission("server-templates.read") {
        return Ok(err.into_response());
    }

    let template = ServerTemplate::by_uuid(&state.database, match uuid::Uuid::parse_str(&server_template[0]) {
        Ok(uuid) => uuid,
        Err(_) => {
            return Ok(ApiResponse::error("server template not found")
                .with_status(StatusCode::NOT_FOUND)
                .into_response());
        }
    })
    .await;
    let template = match template {
        Ok(template) => template,
        Err(_) => {
            return Ok(ApiResponse::error("server template not found")
                .with_status(StatusCode::NOT_FOUND)
                .into_response());
        }
    };

    req.extensions_mut().insert(template);

    Ok(next.run(req).await)
}

mod get {
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoAdminApiObject, user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        server_template: shared::models::server_template::AdminApiServerTemplate,
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
        permissions: GetPermissionManager,
        template: super::GetServerTemplate,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("server-templates.read")?;

        ApiResponse::new_serialized(Response {
            server_template: template
                .0
                .into_admin_api_object(&state, &state.storage.retrieve_urls().await?)
                .await?,
        })
        .ok()
    }
}

mod delete {
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
            "server_template" = uuid::Uuid,
            description = "The server template ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        template: super::GetServerTemplate,
        activity_logger: GetAdminActivityLogger,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("server-templates.delete")?;

        template.0.delete(&state, ()).await?;

        activity_logger
            .log(
                "server-template:delete",
                serde_json::json!({
                    "uuid": template.uuid,
                    "name": template.name,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod patch {
    use axum::http::StatusCode;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            UpdatableModel, admin_activity::GetAdminActivityLogger,
            server_template::UpdateServerTemplateOptions,
            user::GetPermissionManager,
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
        (status = CONFLICT, body = ApiError),
    ), params(
        (
            "server_template" = uuid::Uuid,
            description = "The server template ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(UpdateServerTemplateOptions))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        mut template: super::GetServerTemplate,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<UpdateServerTemplateOptions>,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("server-templates.update")?;

        match template.update(&state, data).await {
            Ok(_) => {}
            Err(err) if err.is_unique_violation() => {
                return ApiResponse::error("a server template with this name already exists")
                    .with_status(StatusCode::CONFLICT)
                    .ok();
            }
            Err(err) => return ApiResponse::from(err).ok(),
        }

        activity_logger
            .log(
                "server-template:update",
                serde_json::json!({
                    "uuid": template.uuid,
                    "name": template.name,
                    "description": template.description,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {}).ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(delete::route))
        .routes(routes!(patch::route))
        .nest("/instantiate", instantiate::router(state))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state.clone())
}
