//! "Restart on crash" watcher (console feature 8).
//!
//! Servers with `servers.restart_on_crash` set watch for Wings crash records
//! and restart themselves after a crash, with backoff: up to
//! [`MAX_RESTARTS`] restarts per crash, waiting 10s, then 1m, then 5m after
//! the crash before trying. After the third failed attempt the server is left
//! offline and the failure is surfaced — never silently retried forever.
//!
//! How attempts are counted (no new tables, no new cache keys): every
//! restart the watcher sends is written to the server activity log as
//! `server:crash.restart` with the Wings crash id in its data, and giving up
//! is written as `server:crash.restart-exhausted`. A new crash gets a new
//! Wings crash id, which resets the count. The activity log is also where a
//! user (or the Feature 1 crash-alert webhooks, which should subscribe to
//! these two events) can see what the watcher did — this file deliberately
//! contains no webhook code of its own.
//!
//! Safety guards, in order:
//! 1. Only servers with the flag on, not suspended, and not installing /
//!    restoring / transferring (the client-API guard already blocks those
//!    states, and the query below mirrors it with `status IS NULL`).
//! 2. Only crashes from the last 24h — an old crash record must not trigger
//!    a restart long after the fact.
//! 3. Only when Wings currently reports the server as `offline`, so a
//!    server someone already restarted by hand is never restarted again.
//! 4. Only once the backoff delay for the next attempt has elapsed since
//!    the crash.
//!
//! Wiring (see MANIFEST.md): declare `pub mod restart_on_crash;` in
//! `backend/src/lib.rs`, and call `crate::restart_on_crash::register(...)`
//! at the end of `define_background_tasks` in `backend/src/tasks.rs`.

use axum::http::StatusCode;
use shared::models::{
    ByUuid,
    server::Server,
    server_activity::{CreateServerActivityOptions, ServerActivity},
};

/// Restarts attempted per crash before the watcher gives up.
const MAX_RESTARTS: i64 = 3;

/// Delay after the crash before attempt 1, 2 and 3 respectively.
const BACKOFF_SECONDS: [i64; 3] = [10, 60, 300];

/// Crashes older than this are ignored entirely.
const CRASH_FRESHNESS_SECONDS: i64 = 24 * 60 * 60;

pub async fn register(
    background_task_builder: &shared::extensions::background_tasks::BackgroundTaskBuilder,
) {
    background_task_builder
        .add_task("restart_on_crash", async |state| {
            const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

            let mut interval = tokio::time::interval(CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                if let Err(err) = restart_crashed_servers(state).await {
                    tracing::error!("restart-on-crash sweep failed: {err:#?}");
                }
            }
        })
        .await;
}

async fn restart_crashed_servers(state: &shared::State) -> Result<(), anyhow::Error> {
    let server_uuids: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"
        SELECT uuid
        FROM servers
        WHERE restart_on_crash AND NOT suspended AND status IS NULL
        "#,
    )
    .fetch_all(state.database.read())
    .await?;

    for server_uuid in server_uuids {
        restart_crashed_server(state, server_uuid).await;
    }

    Ok(())
}

async fn restart_crashed_server(state: &shared::State, server_uuid: uuid::Uuid) {
    let Ok(Some(server)) = Server::by_uuid_optional_cached(&state.database, server_uuid).await
    else {
        return;
    };

    let Ok(node) = server.node.fetch_cached(&state.database).await else {
        return;
    };
    let Ok(client) = node.api_client(&state.database).await else {
        return;
    };

    let crashes = match client
        .get_servers_server_logs_crashes(server.uuid)
        .await
    {
        Ok(data) => data.crashes,
        Err(wings_api::client::ApiHttpError::Http(
            status @ (StatusCode::NOT_FOUND | StatusCode::EXPECTATION_FAILED),
            _,
        )) => {
            tracing::debug!(
                server = %server.uuid,
                "crash history unavailable, skipping restart-on-crash check"
            );
            return;
        }
        Err(err) => {
            tracing::warn!(
                server = %server.uuid,
                "failed to load crash history for restart-on-crash check: {err:#?}"
            );
            return;
        }
    };

    let Some(latest_crash) = crashes.iter().max_by_key(|crash| crash.timestamp) else {
        return;
    };

    if chrono::Utc::now()
        .signed_duration_since(latest_crash.timestamp)
        .num_seconds()
        > CRASH_FRESHNESS_SECONDS
    {
        return;
    }

    let crash_id = latest_crash.id.to_string();
    let attempts = match count_activity(
        state,
        server.uuid,
        "server:crash.restart",
        &crash_id,
    )
    .await
    {
        Ok(attempts) => attempts,
        Err(err) => {
            tracing::warn!(
                server = %server.uuid,
                "failed to count restart-on-crash attempts: {err:#?}"
            );
            return;
        }
    };

    if attempts >= MAX_RESTARTS {
        if let Ok(exhausted) = count_activity(
            state,
            server.uuid,
            "server:crash.restart-exhausted",
            &crash_id,
        )
        .await
        {
            if exhausted == 0 {
                tracing::error!(
                    server = %server.uuid,
                    crash = %crash_id,
                    "restart-on-crash gave up after {MAX_RESTARTS} attempts, leaving server offline"
                );

                if let Err(err) = ServerActivity::create(
                    state,
                    CreateServerActivityOptions {
                        server_uuid: server.uuid,
                        user_uuid: None,
                        impersonator_uuid: None,
                        api_key_uuid: None,
                        schedule_uuid: None,
                        event: "server:crash.restart-exhausted".into(),
                        ip: None,
                        data: serde_json::json!({
                            "crash_id": crash_id,
                            "attempts": attempts,
                        }),
                        created: None,
                    },
                )
                .await
                {
                    tracing::warn!(
                        server = %server.uuid,
                        "failed to log restart-on-crash exhaustion: {err:#?}"
                    );
                }
            }
        }

        return;
    }

    let backoff_seconds = BACKOFF_SECONDS[attempts as usize];
    if chrono::Utc::now()
        < latest_crash.timestamp + chrono::Duration::seconds(backoff_seconds)
    {
        return;
    }

    // Never restart a server that is already running again (e.g. restarted
    // by hand after the crash). A missing resources entry means the state is
    // unknown, which is also a reason not to act.
    let Ok(resources) = node.fetch_server_resources(&state.database).await else {
        return;
    };
    let Some(resources) = resources.get(&server.uuid) else {
        return;
    };
    if resources.state != wings_api::ServerState::Offline {
        return;
    }

    if let Err(err) = client
        .post_servers_server_power(
            server.uuid,
            &wings_api::servers_server_power::post::RequestBody {
                action: wings_api::ServerPowerAction::Restart,
                wait_seconds: None,
            },
        )
        .await
    {
        // A failed send does not consume an attempt; it is retried on the
        // next tick (and logged each time, so a broken node is visible).
        tracing::warn!(
            server = %server.uuid,
            "failed to send restart-on-crash power action: {err:#?}"
        );
        return;
    }

    tracing::info!(
        server = %server.uuid,
        crash = %crash_id,
        attempt = attempts + 1,
        "restarting server after crash (restart-on-crash)"
    );

    if let Err(err) = ServerActivity::create(
        state,
        CreateServerActivityOptions {
            server_uuid: server.uuid,
            user_uuid: None,
            impersonator_uuid: None,
            api_key_uuid: None,
            schedule_uuid: None,
            event: "server:crash.restart".into(),
            ip: None,
            data: serde_json::json!({
                "crash_id": crash_id,
                "attempt": attempts + 1,
                "max_attempts": MAX_RESTARTS,
                "backoff_seconds": backoff_seconds,
            }),
            created: None,
        },
    )
    .await
    {
        tracing::warn!(
            server = %server.uuid,
            "failed to log restart-on-crash activity: {err:#?}"
        );
    }
}

async fn count_activity(
    state: &shared::State,
    server_uuid: uuid::Uuid,
    event: &str,
    crash_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM server_activities
        WHERE server_uuid = $1 AND event = $2 AND data ->> 'crash_id' = $3
        "#,
    )
    .bind(server_uuid)
    .bind(event)
    .bind(crash_id)
    .fetch_one(state.database.read())
    .await
}
