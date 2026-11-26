use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use axum::http::StatusCode;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            admin_activity::GetAdminActivityLogger,
            server::{AdminApiServer, CloneServerOptions, GetServer, Server},
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Deserialize)]
    pub struct Payload {
        #[serde(flatten)]
        pub options: CloneServerOptions,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        server: AdminApiServer,
        /// True when a file copy was started in the background; the new
        /// server stays in `restoring_backup` status until it finishes.
        files_pending: bool,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = CONFLICT, body = ApiError),
        (status = EXPECTATION_FAILED, body = ApiError),
    ), params(
        (
            "server" = uuid::Uuid,
            description = "The server ID to clone",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        server: GetServer,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data.options) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("servers.create")?;

        let files_pending = data.options.include_files;
        let clone = match Server::clone_server(&state, &server, data.options).await {
            Ok(clone) => clone,
            Err(err) if err.is_unique_violation() => {
                return ApiResponse::error(
                    "a server with the requested allocation(s) already exists",
                )
                .with_status(StatusCode::CONFLICT)
                .ok();
            }
            Err(err) => {
                let (message, status) =
                    shared::response::extract_readable_message(&err).unwrap_or_else(|| {
                        (
                            err.to_string(),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    });

                return ApiResponse::error(message).with_status(status).ok();
            }
        };

        activity_logger
            .log(
                "server:clone",
                serde_json::json!({
                    "source_uuid": server.uuid,
                    "source_name": server.name,
                    "uuid": clone.uuid,
                    "name": clone.name,
                    "node_uuid": clone.node.uuid,
                    "owner_uuid": clone.owner.uuid,
                    "egg_uuid": clone.egg.uuid,
                    "files_pending": files_pending,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {
            server: clone
                .into_admin_api_object(&state, &state.storage.retrieve_urls().await?)
                .await?,
            files_pending,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(post::route))
        .with_state(state.clone())
}
