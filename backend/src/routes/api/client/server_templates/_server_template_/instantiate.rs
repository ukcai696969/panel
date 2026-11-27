use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use super::super::GetServerTemplate;
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            IntoApiObject,
            server::ApiServer,
            user::{GetPermissionManager, GetUser},
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        #[garde(skip)]
        node_uuid: uuid::Uuid,

        #[garde(skip)]
        allocation_uuid: Option<uuid::Uuid>,
        #[garde(skip)]
        allocation_uuids: Vec<uuid::Uuid>,

        #[garde(skip)]
        start_on_completion: bool,

        #[garde(length(chars, min = 1, max = 255))]
        #[schema(min_length = 1, max_length = 255)]
        name: compact_str::CompactString,
        #[garde(length(chars, min = 1, max = 1024))]
        #[schema(min_length = 1, max_length = 1024)]
        description: Option<compact_str::CompactString>,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        server: ApiServer,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = CONFLICT, body = ApiError),
    ), params(
        (
            "server_template" = uuid::Uuid,
            description = "The server template ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        user: GetUser,
        template: GetServerTemplate,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_user_permission("servers.read")?;

        // The new server is always owned by the caller, on a node they pick
        // from the nodes they can see; admins use the admin instantiate
        // endpoint when creating for someone else.
        let server = match template
            .0
            .instantiate(
                &state,
                data.name,
                data.description,
                user.uuid,
                data.node_uuid,
                data.allocation_uuid,
                data.allocation_uuids,
                data.start_on_completion,
            )
            .await
        {
            Ok(server) => server,
            Err(err) if err.is_unique_violation() => {
                return ApiResponse::error(
                    "server with allocation(s) or external id already exists",
                )
                .with_status(StatusCode::CONFLICT)
                .ok();
            }
            Err(err) => return ApiResponse::from(err).ok(),
        };

        ApiResponse::new_serialized(Response {
            server: server.into_api_object(&state, &user).await?,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(post::route))
        .with_state(state.clone())
}
