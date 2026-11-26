use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod _webhook_;

mod get {
    use axum::{extract::Query, http::StatusCode};
    use serde::Serialize;
    use shared::{
        ApiError, GetState,
        models::{
            IntoAdminApiObject, Pagination, PaginationParamsWithSearch,
            server_webhook::ServerWebhook, user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        webhooks: Pagination<shared::models::server_webhook::AdminApiServerWebhook>,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
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

        permissions.has_admin_permission("webhooks.read")?;

        let webhooks = ServerWebhook::all_global_with_pagination(
            &state.database,
            params.page,
            params.per_page,
            params.search.as_deref(),
        )
        .await?;

        ApiResponse::new_serialized(Response {
            webhooks: webhooks
                .try_async_map(|webhook| webhook.into_admin_api_object(&state, ()))
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
            CreatableModel, IntoAdminApiObject,
            server_webhook::{CreateServerWebhookOptions, ServerWebhook, ServerWebhookEvent, ServerWebhookKind},
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        #[garde(length(chars, min = 1, max = 255))]
        #[schema(min_length = 1, max_length = 255)]
        name: compact_str::CompactString,
        #[garde(skip)]
        kind: ServerWebhookKind,
        #[garde(length(chars, min = 1, max = 2048))]
        #[schema(min_length = 1, max_length = 2048)]
        url: String,
        #[garde(length(min = 1, max = 8))]
        #[schema(max_length = 8)]
        events: Vec<ServerWebhookEvent>,
        #[garde(skip)]
        enabled: bool,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        webhook: shared::models::server_webhook::AdminApiServerWebhook,
        /// The HMAC signing secret for generic webhooks. Shown exactly
        /// once — it is stored encrypted and cannot be read back.
        /// Empty for Discord webhooks, which need no secret.
        secret: String,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = UNAUTHORIZED, body = ApiError),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("webhooks.create")?;

        let url = match shared::webhooks::validate_webhook_url(&state.env, &data.url) {
            Ok(url) => url,
            Err(err) => {
                return ApiResponse::error(err)
                    .with_status(StatusCode::BAD_REQUEST)
                    .ok();
            }
        };
        if data.kind == ServerWebhookKind::Discord
            && let Err(err) = shared::webhooks::validate_discord_url(&url)
        {
            return ApiResponse::error(err)
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        // Global webhook: server_uuid is None, so the crash watcher
        // delivers it for every server.
        let (secret, webhook) = ServerWebhook::create(
            &state,
            CreateServerWebhookOptions {
                server_uuid: None,
                name: data.name,
                kind: data.kind,
                url: url.to_string().into(),
                events: data.events,
                enabled: data.enabled,
            },
        )
        .await?;

        ApiResponse::new_serialized(Response {
            webhook: webhook.into_admin_api_object(&state, ()).await?,
            secret,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .routes(routes!(post::route))
        .nest("/{webhook}", _webhook_::router(state))
        .with_state(state.clone())
}
