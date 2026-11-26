use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use axum::{extract::Query, http::StatusCode};
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            IntoApiObject, Pagination, PaginationParamsWithSearch, server::GetServer,
            server_activity::ServerActivity,
            user::{GetPermissionManager, GetUser},
        },
        prelude::AsyncIteratorExt,
        response::{ApiResponse, ApiResponseResult},
        settings::activity::ActivityIpHiding,
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {
        #[schema(inline)]
        installs: Pagination<shared::models::server_activity::ApiServerActivity>,
    }

    /// Installed add-ons, derived from the server activity log: every
    /// install records a `server:addon.install` event (see `install.rs`),
    /// so this list is the install history. It deliberately reuses the
    /// activity query — `search` matches the event name (`ILIKE`) in
    /// `ServerActivity::by_server_uuid_with_pagination` — instead of
    /// inventing a new table. Entries are therefore only as durable as
    /// the activity log (pruned by the panel's activity retention).
    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
    ), params(
        (
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
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
    ))]
    pub async fn route(
        state: GetState,
        user: GetUser,
        permissions: GetPermissionManager,
        server: GetServer,
        Query(pagination): Query<PaginationParamsWithSearch>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&pagination) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_server_permission("files.read")?;

        let activities = ServerActivity::by_server_uuid_with_pagination(
            &state.database,
            server.uuid,
            pagination.page,
            pagination.per_page,
            Some("server:addon.install"),
        )
        .await?;

        let storage_url_retriever = state.storage.retrieve_urls().await?;
        let can_read_ip = permissions
            .has_server_permission("activity.read-ip")
            .is_ok();
        let hide_ips = state
            .settings
            .get_as(|s| s.activity.server_hide_activity_ips)
            .await?;

        ApiResponse::new_serialized(Response {
            installs: activities
                .try_async_map(|activity| {
                    let is_own_activity = activity.impersonator.is_none()
                        && activity.user.as_ref().is_some_and(|u| u.uuid == user.uuid);
                    let show_ip = can_read_ip
                        && match hide_ips {
                            ActivityIpHiding::None => true,
                            ActivityIpHiding::Admins => {
                                is_own_activity
                                    || !(activity.impersonator.is_some()
                                        || activity.user.as_ref().is_some_and(|u| u.admin))
                            }
                            ActivityIpHiding::AllUsers => is_own_activity,
                        };

                    activity.into_api_object(&state, (&storage_url_retriever, show_ip))
                })
                .await?,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
