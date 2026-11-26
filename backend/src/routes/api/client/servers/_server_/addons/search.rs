use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use axum::{extract::Query, http::StatusCode};
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            PaginationParams, server::GetServer, server_variable::ServerVariable,
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    use super::super::{
        curseforge, modrinth,
        types::{AddonProject, AddonSource, detect_context},
    };

    #[derive(ToSchema, Deserialize)]
    pub struct Params {
        #[serde(default)]
        query: String,
        /// `mod`, `modpack`, `plugin`, `datapack`, `shader`, `resourcepack`.
        /// Empty means all types.
        #[serde(default)]
        project_type: Option<String>,
        /// `modrinth` (default) or `curseforge`.
        #[serde(default = "default_source")]
        source: String,
    }

    fn default_source() -> String {
        "modrinth".to_string()
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        projects: Vec<AddonProject>,
        total: u64,
        page: i64,
        per_page: i64,
        /// The game version / loaders the results were filtered by, if known.
        context: ContextResponse,
        source: AddonSource,
    }

    #[derive(ToSchema, Serialize)]
    struct ContextResponse {
        is_minecraft: bool,
        game_version: Option<String>,
        loaders: Vec<String>,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = UNAUTHORIZED, body = ApiError),
        (status = BAD_GATEWAY, body = ApiError),
    ), params(
        (
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "query" = String, Query,
            description = "Search term",
            example = "worldedit",
        ),
        (
            "project_type" = Option<String>, Query,
            description = "Filter by project type: mod, modpack, plugin, datapack, shader, resourcepack",
            example = "mod",
        ),
        (
            "source" = String, Query,
            description = "Catalogue source: modrinth (default) or curseforge (needs CURSEFORGE_API_KEY)",
            example = "modrinth",
        ),
        (
            "page" = i64, Query,
            description = "The page number",
            example = "1",
        ),
        (
            "per_page" = i64, Query,
            description = "The number of items per page",
            example = "20",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        server: GetServer,
        Query(pagination): Query<PaginationParams>,
        Query(params): Query<Params>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&pagination) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_server_permission("files.read")?;

        let source = match AddonSource::from_str(&params.source) {
            Some(source) => source,
            None => {
                return ApiResponse::error("unknown source, expected 'modrinth' or 'curseforge'")
                    .with_status(StatusCode::BAD_REQUEST)
                    .ok();
            }
        };

        let variables = ServerVariable::all_by_server_uuid_egg_uuid(
            &state.database,
            server.uuid,
            server.egg.uuid,
        )
        .await?;
        let context = detect_context(&server, &variables);

        let result = match source {
            AddonSource::Modrinth => modrinth::search_projects(
                &state,
                &params.query,
                params.project_type.as_deref(),
                &context,
                pagination.page as u64,
                pagination.per_page as u64,
            )
            .await
            .map_err(|err| (err, StatusCode::BAD_GATEWAY)),
            AddonSource::Curseforge => curseforge::search_projects(
                &state,
                &params.query,
                params.project_type.as_deref(),
                &context,
                pagination.page as u64,
                pagination.per_page as u64,
            )
            .await
            .map_err(|err| {
                // A missing CurseForge key is a configuration problem, not
                // an upstream failure.
                if err.contains("not configured") {
                    (err, StatusCode::SERVICE_UNAVAILABLE)
                } else {
                    (err, StatusCode::BAD_GATEWAY)
                }
            }),
        };

        let result = match result {
            Ok(result) => result,
            Err((err, status)) => {
                return ApiResponse::error(&err).with_status(status).ok();
            }
        };

        ApiResponse::new_serialized(Response {
            projects: result.projects,
            total: result.total,
            page: pagination.page,
            per_page: pagination.per_page,
            context: ContextResponse {
                is_minecraft: context.is_minecraft,
                game_version: context.game_version,
                loaders: context.loaders,
            },
            source,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
