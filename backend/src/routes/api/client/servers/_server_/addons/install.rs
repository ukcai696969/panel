use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            server::{GetServer, GetServerActivityLogger, server_variable::ServerVariable},
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use std::path::Path;
    use utoipa::ToSchema;

    use super::super::{
        curseforge, modrinth,
        types::{
            AddonSource, AddonVersion, destination_for, detect_context, sanitize_file_name,
            validate_destination, validate_download_url,
        },
    };

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        /// `modrinth` (default) or `curseforge`.
        #[garde(skip)]
        #[serde(default = "default_source")]
        source: String,
        /// Modrinth project id/slug, or CurseForge numeric mod id.
        #[garde(length(chars, min = 1, max = 255))]
        project_id: String,
        /// The version id returned by the versions endpoint.
        #[garde(length(chars, min = 1, max = 255))]
        version_id: String,
        /// Project type (`mod`, `plugin`, `modpack`, ...). Used only to pick
        /// the destination folder; the version is re-fetched from the
        /// catalogue, so a lying client cannot change what is installed.
        #[garde(length(chars, min = 1, max = 32))]
        project_type: String,
        /// Optional file name to install when a version ships several files.
        /// Must match one of the version's files exactly.
        #[garde(skip)]
        file_name: Option<String>,
        /// Optional destination override. Must be an absolute path without
        /// `..`; defaults to the folder for the project type.
        #[garde(skip)]
        destination: Option<String>,
        #[garde(skip)]
        #[serde(default)]
        foreground: bool,
    }

    fn default_source() -> String {
        "modrinth".to_string()
    }

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[derive(ToSchema, Serialize)]
    struct ResponseAccepted {
        identifier: uuid::Uuid,
    }

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = ACCEPTED, body = inline(ResponseAccepted)),
        (status = BAD_REQUEST, body = ApiError),
        (status = UNAUTHORIZED, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = EXPECTATION_FAILED, body = ApiError),
        (status = BAD_GATEWAY, body = ApiError),
        (status = SERVICE_UNAVAILABLE, body = ApiError),
    ), params(
        (
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        mut server: GetServer,
        activity_logger: GetServerActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_server_permission("files.create")?;

        // Reuse the files/pull rate limit: installing is a pull by another
        // name, and this keeps a user from queueing unbounded downloads.
        let ratelimit = state
            .settings
            .get_as(|s| s.ratelimits.client_servers_files_pull)
            .await?;
        state
            .cache
            .ratelimit(
                "client/servers/addons/install",
                ratelimit.hits,
                ratelimit.window_seconds,
                server.uuid.to_string(),
            )
            .await?;

        let source = match AddonSource::from_str(&data.source) {
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

        // Re-fetch the version from the catalogue: the client only names a
        // version id, it never supplies a download URL, so a tampered
        // request cannot redirect the install at an arbitrary file.
        let version: AddonVersion = match source {
            AddonSource::Modrinth => {
                // The version id alone identifies the version; `project_id`
                // is only used for the activity log, so a slug vs. id
                // mismatch here must not reject the install.
                modrinth::project_version(&state, &data.version_id, &context)
                    .await
                    .map_err(|err| (err, StatusCode::BAD_GATEWAY))?
            }
            AddonSource::Curseforge => curseforge::project_version(
                &state,
                &data.project_id,
                &data.version_id,
                &context,
            )
            .await
            .map_err(|err| {
                if err.contains("not configured") {
                    (err, StatusCode::SERVICE_UNAVAILABLE)
                } else {
                    (err, StatusCode::BAD_GATEWAY)
                }
            })?,
        };

        if !version.compatible {
            return ApiResponse::error(
                "that version does not match this server's game version or loader",
            )
            .with_status(StatusCode::EXPECTATION_FAILED)
            .ok();
        }

        let file = match &data.file_name {
            Some(name) => match version.files.iter().find(|file| &file.filename == name) {
                Some(file) => file,
                None => {
                    return ApiResponse::error("file is not part of the selected version")
                        .with_status(StatusCode::BAD_REQUEST)
                        .ok();
                }
            },
            None => match version
                .files
                .iter()
                .find(|file| file.primary)
                .or_else(|| version.files.first())
            {
                Some(file) => file,
                None => {
                    return ApiResponse::error("selected version has no downloadable file")
                        .with_status(StatusCode::EXPECTATION_FAILED)
                        .ok();
                }
            },
        };

        let file_name = match sanitize_file_name(&file.filename) {
            Ok(name) => name,
            Err(err) => return ApiResponse::error(&err).with_status(StatusCode::BAD_REQUEST).ok(),
        };

        let destination = match &data.destination {
            Some(destination) => match validate_destination(destination) {
                Ok(destination) => destination,
                Err(err) => {
                    return ApiResponse::error(&err).with_status(StatusCode::BAD_REQUEST).ok();
                }
            },
            None => destination_for(&data.project_type).to_string(),
        };

        if server.is_ignored(Path::new(&destination).join(&file_name), false) {
            return ApiResponse::error("destination not found")
                .with_status(StatusCode::NOT_FOUND)
                .ok();
        }

        // SSRF check on the catalogue-supplied download URL (allowlisted
        // host + HEAD through the blocked-CIDR resolver) before Wings is
        // asked to fetch it.
        if let Err(err) = validate_download_url(&state, source, &file.url).await {
            return ApiResponse::error(&err).with_status(StatusCode::BAD_REQUEST).ok();
        }

        // Install via the existing Wings file-pull API — the same call the
        // Files > Pull URL feature uses. Wings downloads the file straight
        // onto the node, into the destination folder; the panel never
        // proxies the bytes itself.
        let request_body = wings_api::servers_server_files_pull::post::RequestBody {
            root: destination.clone().into(),
            url: file.url.clone().into(),
            file_name: Some(file_name.clone().into()),
            use_header: false,
            foreground: data.foreground,
        };

        let install_log = serde_json::json!({
            "source": source.as_str(),
            "project_id": data.project_id,
            "project_type": data.project_type,
            "version_id": data.version_id,
            "version_number": version.version_number,
            "file": file_name,
            "destination": destination,
        });

        tokio::spawn(async move {
            let identifier = match server
                .0
                .node
                .fetch_cached(&state.database)
                .await?
                .api_client(&state.database)
                .await?
                .ignoring(server.0.subuser_ignored_files.unwrap_or_default())
                .post_servers_server_files_pull(server.0.uuid, &request_body)
                .await
            {
                Ok(wings_api::servers_server_files_pull::post::Response::Ok(_)) => None,
                Ok(wings_api::servers_server_files_pull::post::Response::Accepted(data)) => {
                    Some(data.identifier)
                }
                Err(wings_api::client::ApiHttpError::Http(StatusCode::NOT_FOUND, err)) => {
                    return ApiResponse::new_serialized(ApiError::new_wings_value(err))
                        .with_status(StatusCode::NOT_FOUND)
                        .ok();
                }
                Err(wings_api::client::ApiHttpError::Http(StatusCode::EXPECTATION_FAILED, err)) => {
                    return ApiResponse::new_serialized(ApiError::new_wings_value(err))
                        .with_status(StatusCode::EXPECTATION_FAILED)
                        .ok();
                }
                Err(err) => return Err(err.into()),
            };

            activity_logger
                .log("server:addon.install", install_log)
                .await;

            if let Some(identifier) = identifier {
                ApiResponse::new_serialized(ResponseAccepted { identifier })
                    .with_status(StatusCode::ACCEPTED)
                    .ok()
            } else {
                ApiResponse::new_serialized(Response {}).ok()
            }
        })
        .await?
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(post::route))
        .with_state(state.clone())
}
