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
            user::GetPermissionManager,
            webhook_subscription::WebhookSubscription,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        webhooks: Pagination<shared::models::webhook_subscription::AdminApiWebhookSubscription>,
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

        permissions.has_admin_permission("webhooks.read")?;

        let webhooks = WebhookSubscription::all_with_pagination(
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
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        crypt::EncryptedString,
        models::{
            CreatableModel, IntoAdminApiObject,
            admin_activity::GetAdminActivityLogger,
            user::GetPermissionManager,
            webhook_subscription::{CreateWebhookSubscriptionOptions, WebhookSubscription},
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    /// The create body. The signing secret is generated server-side; it is
    /// not accepted from the client.
    #[derive(ToSchema, Deserialize)]
    pub struct CreateWebhookSubscriptionOptionsWithoutSecret {
        pub name: compact_str::CompactString,
        pub url: compact_str::CompactString,
        pub events: Vec<String>,
        pub enabled: bool,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        webhook: shared::models::webhook_subscription::AdminApiWebhookSubscription,
        /// The plaintext signing secret. Shown exactly once — it is never
        /// stored in plaintext and no read endpoint returns it again.
        secret: String,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
    ), request_body = inline(CreateWebhookSubscriptionOptionsWithoutSecret))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<CreateWebhookSubscriptionOptionsWithoutSecret>,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("webhooks.create")?;

        let secret = WebhookSubscription::generate_secret();
        let options = CreateWebhookSubscriptionOptions {
            name: data.name,
            url: data.url,
            secret: EncryptedString::from_plaintext(&secret, &state.database).await?,
            events: data.events,
            enabled: data.enabled,
            created_by_uuid: Some(permissions.user_uuid()),
        };

        // Validate the client-supplied fields through the model's own rules
        // (URL scheme, known event names) before anything is stored.
        if let Err(errors) = shared::utils::validate_data(&options) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(axum::http::StatusCode::BAD_REQUEST)
                .ok();
        }

        let webhook = WebhookSubscription::create(&state, options).await?;

        activity_logger
            .log(
                "webhook:create",
                serde_json::json!({
                    "uuid": webhook.uuid,
                    "name": webhook.name,
                    "url": webhook.url,
                    "events": webhook.events,
                    "enabled": webhook.enabled,
                }),
            )
            .await;

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
