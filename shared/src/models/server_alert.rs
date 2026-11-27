use crate::{
    models::InsertQueryBuilder,
    prelude::*,
};
use garde::Validate;
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow, prelude::Type};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};
use utoipa::ToSchema;

/// What the crash watcher concluded about a server.
#[derive(ToSchema, Serialize, Deserialize, Type, PartialEq, Eq, Hash, Clone, Copy)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "server_alert_kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerAlertKind {
    /// Wings recorded a crash (non-clean exit / OOM) for the server.
    Crashed,
    /// The server went from running to offline with no crash record.
    Offline,
    /// The server is running again after a previous alert.
    Recovered,
}

/// One alert the crash watcher raised. This is the durable log shown
/// in the per-server Alerts tab and the admin Notifications log, and
/// the source of truth for de-duplication: the watcher only creates
/// a row when the Wings crash id (or the offline transition) has not
/// been alerted on before.
#[derive(Serialize, Deserialize, Clone)]
pub struct ServerAlert {
    pub uuid: uuid::Uuid,
    pub server_uuid: uuid::Uuid,

    pub kind: ServerAlertKind,
    /// Wings crash id, when the alert is backed by a Wings crash
    /// record. `None` for offline/recovered alerts.
    pub crash_id: Option<i64>,
    pub exit_code: Option<i32>,
    pub oom_killed: bool,
    /// Tail of the console at detection time, truncated by the
    /// watcher (see `shared::webhooks::MAX_CONSOLE_CHARS`).
    pub last_console: Option<compact_str::CompactString>,

    pub detected_at: chrono::NaiveDateTime,
    pub created: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for ServerAlert {
    const NAME: &'static str = "server_alert";

    fn get_extension_list() -> &'static super::ModelExtensionList {
        static EXTENSIONS: LazyLock<super::ModelExtensionList> =
            LazyLock::new(|| parking_lot::RwLock::new(Vec::new()));

        &EXTENSIONS
    }

    fn get_extension_data(&self) -> &super::ModelExtensionData {
        &self.extension_data
    }

    #[inline]
    fn base_columns(prefix: Option<&str>) -> BTreeMap<&'static str, compact_str::CompactString> {
        let prefix = prefix.unwrap_or_default();

        BTreeMap::from([
            (
                "server_alerts.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "server_alerts.server_uuid",
                compact_str::format_compact!("{prefix}server_uuid"),
            ),
            (
                "server_alerts.kind",
                compact_str::format_compact!("{prefix}kind"),
            ),
            (
                "server_alerts.crash_id",
                compact_str::format_compact!("{prefix}crash_id"),
            ),
            (
                "server_alerts.exit_code",
                compact_str::format_compact!("{prefix}exit_code"),
            ),
            (
                "server_alerts.oom_killed",
                compact_str::format_compact!("{prefix}oom_killed"),
            ),
            (
                "server_alerts.last_console",
                compact_str::format_compact!("{prefix}last_console"),
            ),
            (
                "server_alerts.detected_at",
                compact_str::format_compact!("{prefix}detected_at"),
            ),
            (
                "server_alerts.created",
                compact_str::format_compact!("{prefix}created"),
            ),
        ])
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        Ok(Self {
            uuid: row.try_get(compact_str::format_compact!("{prefix}uuid").as_str())?,
            server_uuid: row
                .try_get(compact_str::format_compact!("{prefix}server_uuid").as_str())?,
            kind: row.try_get(compact_str::format_compact!("{prefix}kind").as_str())?,
            crash_id: row.try_get(compact_str::format_compact!("{prefix}crash_id").as_str())?,
            exit_code: row
                .try_get(compact_str::format_compact!("{prefix}exit_code").as_str())?,
            oom_killed: row
                .try_get(compact_str::format_compact!("{prefix}oom_killed").as_str())?,
            last_console: row
                .try_get(compact_str::format_compact!("{prefix}last_console").as_str())?,
            detected_at: row
                .try_get(compact_str::format_compact!("{prefix}detected_at").as_str())?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }
}

impl ServerAlert {
    pub async fn by_server_uuid_with_pagination(
        database: &crate::database::Database,
        server_uuid: uuid::Uuid,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        super::PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            format!(
                r#"
                FROM server_alerts
                WHERE server_alerts.server_uuid = $1 AND ({search})
                "#,
                search = super::search_sql(
                    2,
                    &["server_alerts.last_console"],
                    &["server_alerts.uuid"],
                ),
            ),
            "server_alerts.detected_at DESC, server_alerts.created DESC, server_alerts.uuid",
        )
        .bind(server_uuid)
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    /// Admin-wide log, newest first, with the server name attached.
    /// Implemented on the admin API object rather than the model row:
    /// see [`AdminServerAlert::all_with_pagination`].
    pub async fn delete_older_than(
        database: &crate::database::Database,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, crate::database::DatabaseError> {
        let result = sqlx::query(
            r#"
            DELETE FROM server_alerts
            WHERE server_alerts.created < $1
            "#,
        )
        .bind(cutoff.naive_utc())
        .execute(database.write())
        .await?;

        Ok(result.rows_affected())
    }
}

#[async_trait::async_trait]
impl IntoApiObject for ServerAlert {
    type ApiObject = ApiServerAlert;
    type ExtraArgs<'a> = ();

    async fn into_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::ApiObject, crate::database::DatabaseError> {
        let api_object = ApiServerAlert::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            ApiServerAlert {
                uuid: self.uuid,
                server_uuid: self.server_uuid,
                kind: self.kind,
                crash_id: self.crash_id,
                exit_code: self.exit_code,
                oom_killed: self.oom_killed,
                last_console: self.last_console,
                detected_at: self.detected_at.and_utc(),
                created: self.created.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}

#[async_trait::async_trait]
impl ByUuid for ServerAlert {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_alerts
            WHERE server_alerts.uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(uuid)
        .fetch_one(database.read())
        .await?;

        Self::map(None, &row)
    }

    async fn by_uuid_with_transaction(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_alerts
            WHERE server_alerts.uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(uuid)
        .fetch_one(&mut **transaction)
        .await?;

        Self::map(None, &row)
    }
}

#[derive(ToSchema, Deserialize, Validate)]
pub struct CreateServerAlertOptions {
    #[garde(skip)]
    pub server_uuid: uuid::Uuid,
    #[garde(skip)]
    pub kind: ServerAlertKind,
    #[garde(skip)]
    pub crash_id: Option<i64>,
    #[garde(skip)]
    pub exit_code: Option<i32>,
    #[garde(skip)]
    pub oom_killed: bool,
    #[garde(length(chars, max = 4096))]
    #[schema(max_length = 4096)]
    pub last_console: Option<compact_str::CompactString>,
    #[garde(skip)]
    pub detected_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait::async_trait]
impl CreatableModel for ServerAlert {
    type CreateOptions<'a> = CreateServerAlertOptions;
    type CreateResult = Self;

    fn get_create_handlers() -> &'static LazyLock<CreateListenerList<Self>> {
        static CREATE_LISTENERS: LazyLock<CreateListenerList<ServerAlert>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &CREATE_LISTENERS
    }

    async fn create_with_transaction(
        state: &crate::State,
        mut options: Self::CreateOptions<'_>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, crate::database::DatabaseError> {
        options.validate()?;

        let mut query_builder = InsertQueryBuilder::new("server_alerts");

        Self::run_create_handlers(&mut options, &mut query_builder, state, transaction).await?;

        query_builder
            .set("server_uuid", options.server_uuid)
            .set("kind", options.kind)
            .set("crash_id", options.crash_id)
            .set("exit_code", options.exit_code)
            .set("oom_killed", options.oom_killed)
            .set("last_console", options.last_console.as_ref());

        if let Some(detected_at) = options.detected_at {
            query_builder.set("detected_at", detected_at.naive_utc());
        }

        let row = query_builder
            .returning(&Self::columns_sql(None))
            .fetch_one(&mut **transaction)
            .await?;
        let mut alert = Self::map(None, &row)?;

        Self::run_after_create_handlers(&mut alert, &options, state, transaction).await?;

        Ok(alert)
    }
}

#[schema_extension_derive::extendible]
#[init_args(ServerAlert, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "ApiServerAlert")]
pub struct ApiServerAlert {
    pub uuid: uuid::Uuid,
    pub server_uuid: uuid::Uuid,

    pub kind: ServerAlertKind,
    pub crash_id: Option<i64>,
    pub exit_code: Option<i32>,
    pub oom_killed: bool,
    pub last_console: Option<compact_str::CompactString>,

    pub detected_at: chrono::DateTime<chrono::Utc>,
    pub created: chrono::DateTime<chrono::Utc>,
}

/// Admin list row: the alert plus the server it belongs to, so the
/// Notifications log is readable without a second lookup per row.
#[derive(ToSchema, Serialize)]
pub struct AdminServerAlert {
    #[serde(flatten)]
    pub alert: ApiServerAlert,
    pub server_name: compact_str::CompactString,
    pub server_uuid_short: String,
}

impl AdminServerAlert {
    pub async fn all_with_pagination(
        state: &crate::State,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        let alerts = super::PaginatedQueryBuilder::new(
            ServerAlert::columns_sql(None),
            format!(
                r#"
                FROM server_alerts
                WHERE ({search})
                "#,
                search = super::search_sql(
                    1,
                    &["server_alerts.last_console"],
                    &["server_alerts.uuid", "server_alerts.server_uuid"],
                ),
            ),
            "server_alerts.detected_at DESC, server_alerts.created DESC, server_alerts.uuid",
        )
        .bind(search)
        .fetch::<ServerAlert>(&state.database, page, per_page)
        .await?;

        let mut rows = Vec::with_capacity(alerts.data.len());
        for alert in alerts.data {
            let server = super::server::Server::by_uuid_optional_cached(
                &state.database,
                alert.server_uuid,
            )
            .await?;
            let (server_name, server_uuid_short) = match server {
                Some(server) => (
                    server.name.clone(),
                    format!("{:08x}", server.uuid_short),
                ),
                None => ("(deleted server)".into(), String::new()),
            };

            rows.push(Self {
                alert: alert.into_api_object(state, ()).await?,
                server_name,
                server_uuid_short,
            });
        }

        Ok(super::Pagination {
            total: alerts.total,
            per_page: alerts.per_page,
            page: alerts.page,
            data: rows,
        })
    }
}

/// Per-server watcher state, keyed by server. This is what makes
/// alerting exactly-once per crash:
///
/// * `last_seen_state` — the Wings state at the previous tick.
/// * `last_crash_id` — the Wings crash id already alerted on.
/// * `last_alerted_at` — when this server last raised an alert of any
///   kind; the watcher will not raise another alert for the same
///   crash id, and will not raise a second offline alert while the
///   server stays offline.
#[derive(Serialize, Deserialize, Clone)]
pub struct ServerAlertState {
    pub server_uuid: uuid::Uuid,
    pub last_seen_state: compact_str::CompactString,
    pub last_seen_at: chrono::NaiveDateTime,
    pub last_crash_id: Option<i64>,
    pub last_alerted_at: Option<chrono::NaiveDateTime>,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for ServerAlertState {
    const NAME: &'static str = "server_alert_state";

    fn get_extension_list() -> &'static super::ModelExtensionList {
        static EXTENSIONS: LazyLock<super::ModelExtensionList> =
            LazyLock::new(|| parking_lot::RwLock::new(Vec::new()));

        &EXTENSIONS
    }

    fn get_extension_data(&self) -> &super::ModelExtensionData {
        &self.extension_data
    }

    #[inline]
    fn base_columns(prefix: Option<&str>) -> BTreeMap<&'static str, compact_str::CompactString> {
        let prefix = prefix.unwrap_or_default();

        BTreeMap::from([
            (
                "server_alert_states.server_uuid",
                compact_str::format_compact!("{prefix}server_uuid"),
            ),
            (
                "server_alert_states.last_seen_state",
                compact_str::format_compact!("{prefix}last_seen_state"),
            ),
            (
                "server_alert_states.last_seen_at",
                compact_str::format_compact!("{prefix}last_seen_at"),
            ),
            (
                "server_alert_states.last_crash_id",
                compact_str::format_compact!("{prefix}last_crash_id"),
            ),
            (
                "server_alert_states.last_alerted_at",
                compact_str::format_compact!("{prefix}last_alerted_at"),
            ),
        ])
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        Ok(Self {
            server_uuid: row
                .try_get(compact_str::format_compact!("{prefix}server_uuid").as_str())?,
            last_seen_state: row
                .try_get(compact_str::format_compact!("{prefix}last_seen_state").as_str())?,
            last_seen_at: row
                .try_get(compact_str::format_compact!("{prefix}last_seen_at").as_str())?,
            last_crash_id: row
                .try_get(compact_str::format_compact!("{prefix}last_crash_id").as_str())?,
            last_alerted_at: row
                .try_get(compact_str::format_compact!("{prefix}last_alerted_at").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }
}

impl ServerAlertState {
    /// The previous tick's state, if the watcher has ever seen this
    /// server. `None` on first sight: the watcher records the state
    /// and does not alert, so a panel restart never mass-alerts.
    pub async fn previous(
        database: &crate::database::Database,
        server_uuid: uuid::Uuid,
    ) -> Result<Option<Self>, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_alert_states
            WHERE server_alert_states.server_uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(server_uuid)
        .fetch_optional(database.read())
        .await?;

        row.try_map(|row| Self::map(None, &row))
    }

    /// Upserts the observed state. When `alerted` is true the row also
    /// records `last_alerted_at = NOW()` and, for crash alerts, the
    /// crash id that was alerted on — atomically, so two concurrent
    /// ticks cannot both claim the same crash.
    pub async fn record_observation(
        database: &crate::database::Database,
        server_uuid: uuid::Uuid,
        state: &str,
        crash_id: Option<i64>,
        alerted: bool,
    ) -> Result<(), crate::database::DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO server_alert_states
                (server_uuid, last_seen_state, last_seen_at, last_crash_id, last_alerted_at)
            VALUES ($1, $2, NOW(), $3, CASE WHEN $4 THEN NOW() ELSE NULL END)
            ON CONFLICT (server_uuid) DO UPDATE SET
                last_seen_state = EXCLUDED.last_seen_state,
                last_seen_at = NOW(),
                last_crash_id = COALESCE(EXCLUDED.last_crash_id, server_alert_states.last_crash_id),
                last_alerted_at = CASE WHEN $4 THEN NOW() ELSE server_alert_states.last_alerted_at END
            "#,
        )
        .bind(server_uuid)
        .bind(state)
        .bind(crash_id)
        .bind(alerted)
        .execute(database.write())
        .await?;

        Ok(())
    }
}
