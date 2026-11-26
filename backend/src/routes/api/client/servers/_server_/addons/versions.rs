use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use axum::{extract::Query, http::StatusCode};
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            server::GetServer, server_variable::ServerVariable, user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    use super::super::{
        curseforge, modrinth,
        types::{AddonSource, AddonVersion, detect_context},
    };

    #[derive(ToSchema, Deserialize)]
    pub struct Params {
        /// Modrinth project id/slug, or CurseForge numeric mod id.
        project_id: String,
        /// `modrinth` (default) or `curseforge`.
        #[serde(default = "default_source")]
        source: String,
    }

    fn default_source() -> String {
        "modrinth".to_string()
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        versions: Vec<AddonVersion>,
        game_version: Option<String>,
        loaders: Vec<String>,
        source: AddonSource,
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
            "project_id" = String, Query,
            description = "The Modrinth project id/slug, or the CurseForge mod id",
            example = "worldedit",
        ),
        (
            "source" = String, Query,
            description = "Catalogue source: modrinth (default) or curseforge",
            example = "modrinth",
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        server: GetServer,
        Query(params): Query<Params>,
    ) -> ApiResponseResult {
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

        let versions = match source {
            AddonSource::Modrinth => modrinth::project_versions(&state, &params.project_id, &context)
                .await
                .map_err(|err| (err, StatusCode::BAD_GATEWAY)),
            AddonSource::Curseforge => {
                curseforge::project_versions(&state, &params.project_id, &context)
                    .await
                    .map_err(|err| {
                        if err.contains("not configured") {
                            (err, StatusCode::SERVICE_UNAVAILABLE)
                        } else {
                            (err, StatusCode::BAD_GATEWAY)
                        }
                    })
            }
        };

        let versions = match versions {
            Ok(versions) => versions,
            Err((err, status)) => {
                return ApiResponse::error(&err).with_status(status).ok();
            }
        };

        ApiResponse::new_serialized(Response {
            versions,
            game_version: context.game_version,
            loaders: context.loaders,
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
