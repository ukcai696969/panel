use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            EventEmittingModel,
            server::{GetServer, GetServerActivityLogger, Server, ServerEvent},
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Deserialize)]
    pub struct Payload {
        #[serde(alias = "signal")]
        action: wings_api::ServerPowerAction,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
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
        server: GetServer,
        activity_logger: GetServerActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        permissions.has_server_permission(match data.action {
            wings_api::ServerPowerAction::Start => "control.start",
            wings_api::ServerPowerAction::Stop => "control.stop",
            wings_api::ServerPowerAction::Kill => "control.stop",
            wings_api::ServerPowerAction::Restart => "control.restart",
        })?;

        tokio::spawn(async move {
            server
                .node
                .fetch_cached(&state.database)
                .await?
                .api_client(&state.database)
                .await?
                .post_servers_server_power(
                    server.uuid,
                    &wings_api::servers_server_power::post::RequestBody {
                        action: data.action,
                        wait_seconds: None,
                    },
                )
                .await?;

            // Wings accepted the power action; the server is (being)
            // started/stopped now. A restart stops then starts.
            match data.action {
                wings_api::ServerPowerAction::Start => {
                    Server::get_event_emitter().emit(
                        state.clone(),
                        ServerEvent::Started {
                            server: Box::new(server.0.clone()),
                        },
                    );
                }
                wings_api::ServerPowerAction::Stop | wings_api::ServerPowerAction::Kill => {
                    Server::get_event_emitter().emit(
                        state.clone(),
                        ServerEvent::Stopped {
                            server: Box::new(server.0.clone()),
                        },
                    );
                }
                wings_api::ServerPowerAction::Restart => {
                    Server::get_event_emitter().emit(
                        state.clone(),
                        ServerEvent::Stopped {
                            server: Box::new(server.0.clone()),
                        },
                    );
                    Server::get_event_emitter().emit(
                        state.clone(),
                        ServerEvent::Started {
                            server: Box::new(server.0.clone()),
                        },
                    );
                }
            }

            activity_logger
                .log(
                    "server:power.action",
                    serde_json::json!({
                        "action": data.action
                    }),
                )
                .await;

            ApiResponse::new_serialized(Response {}).ok()
        })
        .await?
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(post::route))
        .with_state(state.clone())
}
