use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod _server_template_;

mod get {
    use axum::{extract::Query, http::StatusCode};
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoAdminApiObject, Pagination, PaginationParamsWithSearch,
            server_template::ServerTemplate, user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        server_templates: Pagination<shared::models::server_template::AdminApiServerTemplate>,
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
        Query(params): Query<PaginationParamsWithSearch>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("server-templates.read")?;

        let templates = ServerTemplate::all_with_pagination(
            &state.database,
            params.page,
            params.per_page,
            params.search.as_deref(),
        )
        .await?;

        let storage_url_retriever = state.storage.retrieve_urls().await?;

        ApiResponse::new_serialized(Response {
            server_templates: templates
                .try_async_map(|template| {
                    template.into_admin_api_object(&state, &storage_url_retriever)
                })
                .await?,
        })
        .ok()
    }
}

mod post {
    use axum::http::StatusCode;
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            CreatableModel, IntoAdminApiObject,
            admin_activity::GetAdminActivityLogger,
            server_template::{CreateServerTemplateOptions, ServerTemplate},
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        server_template: shared::models::server_template::AdminApiServerTemplate,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = CONFLICT, body = ApiError),
    ), request_body = inline(CreateServerTemplateOptions))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<CreateServerTemplateOptions>,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("server-templates.create")?;

        // A global template (owner_uuid None) is admin-only by definition;
        // a personal template for another user is not something an admin
        // creates through this endpoint either — admins save their own.
        let template = match ServerTemplate::create(&state, data).await {
            Ok(template) => template,
            Err(err) if err.is_unique_violation() => {
                return ApiResponse::error(
                    "a server template with this name already exists for this owner",
                )
                .with_status(StatusCode::CONFLICT)
                .ok();
            }
            Err(err) => return ApiResponse::from(err).ok(),
        };

        activity_logger
            .log(
                "server-template:create",
                serde_json::json!({
                    "uuid": template.uuid,
                    "name": template.name,
                    "source_server_uuid": template.source_server_uuid,
                    "egg_uuid": template.snapshot.egg_uuid,
                    "owner_uuid": template.owner_uuid,
                }),
            )
            .await;

        ApiResponse::new_serialized(Response {
            server_template: template
                .into_admin_api_object(&state, &state.storage.retrieve_urls().await?)
                .await?,
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
