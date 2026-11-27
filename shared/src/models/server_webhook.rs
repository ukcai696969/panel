use crate::{
    crypt::EncryptedString,
    models::{InsertQueryBuilder, UpdateQueryBuilder},
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

/// Where a webhook delivers to.
#[derive(ToSchema, Serialize, Deserialize, Type, PartialEq, Eq, Hash, Clone, Copy)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "server_webhook_kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerWebhookKind {
    /// A Discord incoming-webhook URL. Receives a rich embed.
    Discord,
    /// Any other HTTPS endpoint. Receives a signed JSON payload.
    Generic,
}

/// Which panel events a webhook is subscribed to.
#[derive(ToSchema, Serialize, Deserialize, Type, PartialEq, Eq, Hash, Clone, Copy)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "server_webhook_event", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServerWebhookEvent {
    /// Wings recorded a crash for the server (non-clean exit / OOM).
    ServerCrashed,
    /// A server that was running went offline with no crash record
    /// (e.g. the node lost it). Only raised after a crash check finds
    /// no matching crash entry, so intentional stops never fire it.
    ServerOffline,
    /// A server that previously alerted is running again.
    ServerRecovered,
}

impl ServerWebhookEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServerCrashed => "server.crashed",
            Self::ServerOffline => "server.offline",
            Self::ServerRecovered => "server.recovered",
        }
    }
}

/// A webhook subscription. `server_uuid` is `None` for a global
/// (admin) webhook, which fires for every server; otherwise it fires
/// only for that one server.
///
/// The `url` itself is stored in plaintext (a Discord webhook URL is
/// already a bearer secret — treat the row accordingly), while
/// `secret` — the HMAC key used to sign generic-webhook payloads —
/// is stored encrypted and is only ever shown once, at creation time.
#[derive(Serialize, Deserialize, Clone)]
pub struct ServerWebhook {
    pub uuid: uuid::Uuid,
    /// `None` = global webhook (admin-managed, all servers).
    pub server_uuid: Option<uuid::Uuid>,

    pub name: compact_str::CompactString,
    pub kind: ServerWebhookKind,
    pub url: compact_str::CompactString,
    pub events: Vec<ServerWebhookEvent>,
    pub enabled: bool,

    /// HMAC signing secret, encrypted at rest. `None` for Discord
    /// webhooks, which do not use signing.
    pub secret: Option<EncryptedString>,

    pub last_triggered_at: Option<chrono::NaiveDateTime>,
    pub failure_count: i32,

    pub created: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for ServerWebhook {
    const NAME: &'static str = "server_webhook";

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
                "server_webhooks.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "server_webhooks.server_uuid",
                compact_str::format_compact!("{prefix}server_uuid"),
            ),
            (
                "server_webhooks.name",
                compact_str::format_compact!("{prefix}name"),
            ),
            (
                "server_webhooks.kind",
                compact_str::format_compact!("{prefix}kind"),
            ),
            (
                "server_webhooks.url",
                compact_str::format_compact!("{prefix}url"),
            ),
            (
                "server_webhooks.events",
                compact_str::format_compact!("{prefix}events"),
            ),
            (
                "server_webhooks.enabled",
                compact_str::format_compact!("{prefix}enabled"),
            ),
            (
                "server_webhooks.secret",
                compact_str::format_compact!("{prefix}secret"),
            ),
            (
                "server_webhooks.last_triggered_at",
                compact_str::format_compact!("{prefix}last_triggered_at"),
            ),
            (
                "server_webhooks.failure_count",
                compact_str::format_compact!("{prefix}failure_count"),
            ),
            (
                "server_webhooks.created",
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
            name: row.try_get(compact_str::format_compact!("{prefix}name").as_str())?,
            kind: row.try_get(compact_str::format_compact!("{prefix}kind").as_str())?,
            url: row.try_get(compact_str::format_compact!("{prefix}url").as_str())?,
            events: row.try_get(compact_str::format_compact!("{prefix}events").as_str())?,
            enabled: row.try_get(compact_str::format_compact!("{prefix}enabled").as_str())?,
            secret: row.try_get(compact_str::format_compact!("{prefix}secret").as_str())?,
            last_triggered_at: row
                .try_get(compact_str::format_compact!("{prefix}last_triggered_at").as_str())?,
            failure_count: row
                .try_get(compact_str::format_compact!("{prefix}failure_count").as_str())?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }

    fn cache_invalidation_keys(&self) -> Vec<compact_str::CompactString> {
        vec![compact_str::format_compact!(
            "{}::{}",
            Self::NAME,
            self.uuid
        )]
    }
}

impl ServerWebhook {
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
                FROM server_webhooks
                WHERE server_webhooks.server_uuid = $1 AND ({search})
                "#,
                search = super::search_sql(2, &["server_webhooks.name", "server_webhooks.url"], &["server_webhooks.uuid"]),
            ),
            "server_webhooks.created, server_webhooks.uuid",
        )
        .bind(server_uuid)
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    pub async fn all_global_with_pagination(
        database: &crate::database::Database,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        super::PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            format!(
                r#"
                FROM server_webhooks
                WHERE server_webhooks.server_uuid IS NULL AND ({search})
                "#,
                search = super::search_sql(1, &["server_webhooks.name", "server_webhooks.url"], &["server_webhooks.uuid"]),
            ),
            "server_webhooks.created, server_webhooks.uuid",
        )
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    /// Webhooks that should fire for `server_uuid`: the server's own
    /// enabled webhooks plus the enabled global ones.
    pub async fn enabled_for_server(
        database: &crate::database::Database,
        server_uuid: uuid::Uuid,
    ) -> Result<Vec<Self>, crate::database::DatabaseError> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_webhooks
            WHERE server_webhooks.enabled = true
                AND (server_webhooks.server_uuid = $1 OR server_webhooks.server_uuid IS NULL)
            ORDER BY server_webhooks.created, server_webhooks.uuid
            "#,
            Self::columns_sql(None)
        )))
        .bind(server_uuid)
        .fetch_all(database.read())
        .await?;

        rows.into_iter()
            .map(|row| Self::map(None, &row))
            .try_collect_vec()
    }

    pub async fn by_server_uuid_uuid(
        database: &crate::database::Database,
        server_uuid: uuid::Uuid,
        uuid: uuid::Uuid,
    ) -> Result<Option<Self>, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_webhooks
            WHERE server_webhooks.server_uuid = $1 AND server_webhooks.uuid = $2
            "#,
            Self::columns_sql(None)
        )))
        .bind(server_uuid)
        .bind(uuid)
        .fetch_optional(database.read())
        .await?;

        row.try_map(|row| Self::map(None, &row))
    }

    pub async fn by_uuid_global(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Option<Self>, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_webhooks
            WHERE server_webhooks.server_uuid IS NULL AND server_webhooks.uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(uuid)
        .fetch_optional(database.read())
        .await?;

        row.try_map(|row| Self::map(None, &row))
    }

    /// Records a delivery attempt. Success resets the failure count;
    /// failure increments it (used by the admin log to spot dead URLs).
    pub async fn record_delivery(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
        successful: bool,
    ) -> Result<(), crate::database::DatabaseError> {
        sqlx::query(
            r#"
            UPDATE server_webhooks
            SET last_triggered_at = NOW(),
                failure_count = CASE WHEN $2 THEN 0 ELSE failure_count + 1 END
            WHERE server_webhooks.uuid = $1
            "#,
        )
        .bind(uuid)
        .bind(successful)
        .execute(database.write())
        .await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl IntoApiObject for ServerWebhook {
    type ApiObject = ApiServerWebhook;
    type ExtraArgs<'a> = ();

    async fn into_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::ApiObject, crate::database::DatabaseError> {
        let api_object = ApiServerWebhook::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            ApiServerWebhook {
                uuid: self.uuid,
                server_uuid: self.server_uuid,
                name: self.name,
                kind: self.kind,
                url: self.url,
                events: self.events,
                enabled: self.enabled,
                has_secret: self.secret.is_some(),
                last_triggered_at: self.last_triggered_at.map(|dt| dt.and_utc()),
                failure_count: self.failure_count,
                created: self.created.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}

#[async_trait::async_trait]
impl IntoAdminApiObject for ServerWebhook {
    type AdminApiObject = AdminApiServerWebhook;
    type ExtraArgs<'a> = ();

    async fn into_admin_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        let api_object = AdminApiServerWebhook::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            AdminApiServerWebhook {
                uuid: self.uuid,
                server_uuid: self.server_uuid,
                name: self.name,
                kind: self.kind,
                url: self.url,
                events: self.events,
                enabled: self.enabled,
                has_secret: self.secret.is_some(),
                last_triggered_at: self.last_triggered_at.map(|dt| dt.and_utc()),
                failure_count: self.failure_count,
                created: self.created.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}

#[async_trait::async_trait]
impl ByUuid for ServerWebhook {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_webhooks
            WHERE server_webhooks.uuid = $1
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
            FROM server_webhooks
            WHERE server_webhooks.uuid = $1
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
pub struct CreateServerWebhookOptions {
    #[garde(skip)]
    pub server_uuid: Option<uuid::Uuid>,
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub name: compact_str::CompactString,
    #[garde(skip)]
    pub kind: ServerWebhookKind,
    /// Validated by the route with [`crate::webhooks::validate_webhook_url`]
    /// before this is constructed; the model does not re-check it.
    #[garde(length(chars, min = 1, max = 2048))]
    #[schema(min_length = 1, max_length = 2048)]
    pub url: compact_str::CompactString,
    #[garde(length(min = 1, max = 8))]
    #[schema(max_length = 8)]
    pub events: Vec<ServerWebhookEvent>,
    #[garde(skip)]
    pub enabled: bool,
}

/// The plaintext HMAC secret is generated server-side at creation and
/// returned once alongside the row; it is never readable again.
pub fn generate_webhook_secret() -> String {
    use rand::{RngExt, distr::Alphanumeric};

    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

#[async_trait::async_trait]
impl CreatableModel for ServerWebhook {
    type CreateOptions<'a> = CreateServerWebhookOptions;
    type CreateResult = (String, Self);

    fn get_create_handlers() -> &'static LazyLock<CreateListenerList<Self>> {
        static CREATE_LISTENERS: LazyLock<CreateListenerList<ServerWebhook>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &CREATE_LISTENERS
    }

    async fn create_with_transaction(
        state: &crate::State,
        mut options: Self::CreateOptions<'_>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self::CreateResult, crate::database::DatabaseError> {
        options.validate()?;

        // Discord webhooks carry their own bearer token in the URL and
        // need no signing secret; generic webhooks get one.
        let secret_plaintext = match options.kind {
            ServerWebhookKind::Discord => None,
            ServerWebhookKind::Generic => Some(generate_webhook_secret()),
        };
        let secret = match &secret_plaintext {
            Some(secret) => Some(
                EncryptedString::from_plaintext(secret.clone(), &state.database).await?,
            ),
            None => None,
        };

        let mut query_builder = InsertQueryBuilder::new("server_webhooks");

        Self::run_create_handlers(&mut options, &mut query_builder, state, transaction).await?;

        query_builder
            .set("server_uuid", options.server_uuid)
            .set("name", &options.name)
            .set("kind", options.kind)
            .set("url", &options.url)
            .set("events", &options.events)
            .set("enabled", options.enabled)
            .set("secret", secret.as_ref().map(|s| s.as_ref()));

        let row = query_builder
            .returning(&Self::columns_sql(None))
            .fetch_one(&mut **transaction)
            .await?;
        let webhook = Self::map(None, &row)?;
        let mut result = (secret_plaintext.clone().unwrap_or_default(), webhook);

        Self::run_after_create_handlers(&mut result, &options, state, transaction).await?;

        Ok(result)
    }
}

#[derive(ToSchema, Serialize, Deserialize, Validate, Clone, Default)]
pub struct UpdateServerWebhookOptions {
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub name: Option<compact_str::CompactString>,
    /// Validated by the route with [`crate::webhooks::validate_webhook_url`].
    #[garde(length(chars, min = 1, max = 2048))]
    #[schema(min_length = 1, max_length = 2048)]
    pub url: Option<compact_str::CompactString>,
    #[garde(length(min = 1, max = 8))]
    #[schema(max_length = 8)]
    pub events: Option<Vec<ServerWebhookEvent>>,
    #[garde(skip)]
    pub enabled: Option<bool>,
}

#[async_trait::async_trait]
impl UpdatableModel for ServerWebhook {
    type UpdateOptions = UpdateServerWebhookOptions;

    fn get_update_handlers() -> &'static LazyLock<UpdateHandlerList<Self>> {
        static UPDATE_LISTENERS: LazyLock<UpdateHandlerList<ServerWebhook>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &UPDATE_LISTENERS
    }

    async fn update_with_transaction(
        &mut self,
        state: &crate::State,
        mut options: Self::UpdateOptions,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), crate::database::DatabaseError> {
        options.validate()?;

        let mut query_builder = UpdateQueryBuilder::new("server_webhooks");

        self.run_update_handlers(&mut options, &mut query_builder, state, transaction)
            .await?;

        query_builder
            .set("name", options.name.as_ref())
            .set("url", options.url.as_ref())
            .set("events", options.events.as_ref())
            .set("enabled", options.enabled)
            .where_eq("uuid", self.uuid);

        query_builder.execute(&mut **transaction).await?;

        if let Some(name) = options.name {
            self.name = name;
        }
        if let Some(url) = options.url {
            self.url = url;
        }
        if let Some(events) = options.events {
            self.events = events;
        }
        if let Some(enabled) = options.enabled {
            self.enabled = enabled;
        }

        self.run_after_update_handlers(state, transaction).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl DeletableModel for ServerWebhook {
    type DeleteOptions = ();

    fn get_delete_handlers() -> &'static LazyLock<DeleteHandlerList<Self>> {
        static DELETE_LISTENERS: LazyLock<DeleteHandlerList<ServerWebhook>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &DELETE_LISTENERS
    }

    async fn delete_with_transaction(
        &self,
        state: &crate::State,
        options: Self::DeleteOptions,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), anyhow::Error> {
        self.run_delete_handlers(&options, state, transaction)
            .await?;

        sqlx::query(
            r#"
            DELETE FROM server_webhooks
            WHERE server_webhooks.uuid = $1
            "#,
        )
        .bind(self.uuid)
        .execute(&mut **transaction)
        .await?;

        self.run_after_delete_handlers(&options, state, transaction)
            .await?;

        Ok(())
    }
}

#[schema_extension_derive::extendible]
#[init_args(ServerWebhook, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "ApiServerWebhook")]
pub struct ApiServerWebhook {
    pub uuid: uuid::Uuid,
    pub server_uuid: Option<uuid::Uuid>,

    pub name: compact_str::CompactString,
    pub kind: ServerWebhookKind,
    pub url: compact_str::CompactString,
    pub events: Vec<ServerWebhookEvent>,
    pub enabled: bool,

    /// Whether a signing secret exists (generic webhooks). The secret
    /// itself is never returned by the API after creation.
    pub has_secret: bool,

    pub last_triggered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub failure_count: i32,

    pub created: chrono::DateTime<chrono::Utc>,
}

#[schema_extension_derive::extendible]
#[init_args(ServerWebhook, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "AdminApiServerWebhook")]
pub struct AdminApiServerWebhook {
    pub uuid: uuid::Uuid,
    pub server_uuid: Option<uuid::Uuid>,

    pub name: compact_str::CompactString,
    pub kind: ServerWebhookKind,
    pub url: compact_str::CompactString,
    pub events: Vec<ServerWebhookEvent>,
    pub enabled: bool,

    pub has_secret: bool,

    pub last_triggered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub failure_count: i32,

    pub created: chrono::DateTime<chrono::Utc>,
}
