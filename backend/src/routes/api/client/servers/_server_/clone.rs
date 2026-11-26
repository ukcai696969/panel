use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use axum::http::StatusCode;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            IntoApiObject,
            server::{ApiServer, CloneServerOptions, GetServer, GetServerActivityLogger, Server},
            user::{GetPermissionManager, GetUser},
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
        server: ApiServer,
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
        user: GetUser,
        server: GetServer,
        activity_logger: GetServerActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data.options) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        // Cloning is a scoped duplicate of a server the caller already owns,
        // not a general server-create grant: the owner and node stay the
        // source server's unless the caller is an admin with servers.create.
        let is_admin = permissions
            .has_admin_permission("servers.create")
            .is_ok();
        if server.owner.uuid != user.uuid && !is_admin {
            return ApiResponse::error("only the server owner can clone it")
                .with_status(StatusCode::FORBIDDEN)
                .ok();
        }

        let mut options = data.options;
        if !is_admin {
            options.owner_uuid = None;
            options.node_uuid = None;
            options.allocation_uuid = None;
            options.allocation_uuids = Vec::new();
        }

        let files_pending = options.include_files;
        let clone = match Server::clone_server(&state, &server, options).await {
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
                    "files_pending": files_pending,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {
            server: clone.into_api_object(&state, &user).await?,
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
