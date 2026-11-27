use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod _server_template_;

mod get {
    use axum::{extract::Query, http::StatusCode};
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoApiObject, Pagination, PaginationParamsWithSearch,
            server_template::{ApiServerTemplate, ServerTemplate},
            user::{GetPermissionManager, GetUser},
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        server_templates: Pagination<ApiServerTemplate>,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
    ), params(
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
        (
            "search" = Option<String>, Query,
            description = "Search term for items",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        user: GetUser,
        Query(params): Query<PaginationParamsWithSearch>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_user_permission("servers.read")?;

        let templates = ServerTemplate::visible_by_user_uuid_with_pagination(
            &state.database,
            user.uuid,
            params.page,
            params.per_page,
            params.search.as_deref(),
        )
        .await?;

        ApiResponse::new_serialized(Response {
            server_templates: templates
                .try_async_map(|template| template.into_api_object(&state, ()))
                .await?,
        })
        .ok()
    }
}

mod post {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            CreatableModel, IntoApiObject,
            server_template::{CreateServerTemplateOptions, ServerTemplate},
            user::{GetPermissionManager, GetUser},
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    /// Save one of the caller's own servers as a personal template. The
    /// template is owned by the caller; global templates are admin-only and
    /// are created through the admin API.
    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        #[garde(skip)]
        server_uuid: uuid::Uuid,
        #[garde(length(chars, min = 1, max = 255))]
        #[schema(min_length = 1, max_length = 255)]
        name: compact_str::CompactString,
        #[garde(length(chars, min = 1, max = 1024))]
        #[schema(min_length = 1, max_length = 1024)]
        description: Option<compact_str::CompactString>,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        server_template: shared::models::server_template::ApiServerTemplate,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = CONFLICT, body = ApiError),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        user: GetUser,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        // The server being snapshotted must belong to the caller; reuse the
        // client server lookup, which enforces ownership / subuser access,
        // and then require outright ownership for snapshotting.
        let server = match shared::models::server::Server::by_user_identifier(
            &state.database,
            &permissions,
            &data.server_uuid.to_string(),
        )
        .await
        {
            Ok(Some(server)) => server,
            Ok(None) => {
                return ApiResponse::error("server not found")
                    .with_status(StatusCode::NOT_FOUND)
                    .ok();
            }
            Err(err) => return ApiResponse::from(err).ok(),
        };

        if server.owner.uuid != user.uuid
            && permissions
                .has_admin_permission("server-templates.create")
                .is_err()
        {
            return ApiResponse::error("only the server owner can save it as a template")
                .with_status(StatusCode::FORBIDDEN)
                .ok();
        }

        let template = match ServerTemplate::create(
            &state,
            CreateServerTemplateOptions {
                server_uuid: data.server_uuid,
                name: data.name,
                description: data.description,
                owner_uuid: Some(user.uuid),
            },
        )
        .await
        {
            Ok(template) => template,
            Err(err) if err.is_unique_violation() => {
                return ApiResponse::error("a server template with this name already exists")
                    .with_status(StatusCode::CONFLICT)
                    .ok();
            }
            Err(err) => return ApiResponse::from(err).ok(),
        };

        // No per-server activity logger exists on this route (it is not
        // nested under /{server}); the template row itself records the
        // source server, and admin-created templates are logged server-side
        // in the admin route.
        let _ = server;

        ApiResponse::new_serialized(Response {
            server_template: template.into_api_object(&state, ()).await?,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(post::route))
        .nest("/{server_template}", _server_template_::router(state))
        .with_state(state.clone())
}
