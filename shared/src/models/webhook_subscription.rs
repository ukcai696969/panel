use crate::{
    crypt::EncryptedString,
    models::{InsertQueryBuilder, UpdateQueryBuilder},
    prelude::*,
};
use garde::Validate;
use rand::distr::SampleString;
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};
use utoipa::ToSchema;

/// A webhook URL is only accepted when it uses plain HTTP(S); anything else
/// (e.g. `file://`, `gopher://`, SSRF-adjacent schemes) is rejected here so a
/// stored subscription can never make the panel fetch an internal resource.
pub fn validate_webhook_url(
    url: &compact_str::CompactString,
    _context: &(),
) -> Result<(), garde::Error> {
    if url.len() > 2048 {
        return Err(garde::Error::new("url must be at most 2048 characters"));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        if url.contains(char::is_whitespace) {
            return Err(garde::Error::new("url must not contain whitespace"));
        }

        return Ok(());
    }

    Err(garde::Error::new("url must start with http:// or https://"))
}

pub fn validate_webhook_events(
    events: &[String],
    _context: &(),
) -> Result<(), garde::Error> {
    if events.is_empty() {
        return Err(garde::Error::new(
            "at least one event must be subscribed to",
        ));
    }
    if events.len() > 32 {
        return Err(garde::Error::new("cannot subscribe to more than 32 events"));
    }

    for event in events {
        if !crate::webhooks::is_valid_webhook_event(event) {
            return Err(garde::Error::new(format!("unknown webhook event: {event}")));
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WebhookSubscription {
    pub uuid: uuid::Uuid,

    pub name: compact_str::CompactString,
    pub url: compact_str::CompactString,
    /// HMAC signing secret, encrypted at rest. The plaintext is only ever
    /// returned once, at creation (and rotation) time.
    pub secret: EncryptedString,
    pub events: Vec<String>,
    pub enabled: bool,

    pub created_by_uuid: Option<uuid::Uuid>,

    pub created: chrono::NaiveDateTime,
    pub updated: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for WebhookSubscription {
    const NAME: &'static str = "webhook_subscription";

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
                "webhook_subscriptions.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "webhook_subscriptions.name",
                compact_str::format_compact!("{prefix}name"),
            ),
            (
                "webhook_subscriptions.url",
                compact_str::format_compact!("{prefix}url"),
            ),
            (
                "webhook_subscriptions.secret",
                compact_str::format_compact!("{prefix}secret"),
            ),
            (
                "webhook_subscriptions.events",
                compact_str::format_compact!("{prefix}events"),
            ),
            (
                "webhook_subscriptions.enabled",
                compact_str::format_compact!("{prefix}enabled"),
            ),
            (
                "webhook_subscriptions.created_by_uuid",
                compact_str::format_compact!("{prefix}created_by_uuid"),
            ),
            (
                "webhook_subscriptions.created",
                compact_str::format_compact!("{prefix}created"),
            ),
            (
                "webhook_subscriptions.updated",
                compact_str::format_compact!("{prefix}updated"),
            ),
        ])
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        Ok(Self {
            uuid: row.try_get(compact_str::format_compact!("{prefix}uuid").as_str())?,
            name: row.try_get(compact_str::format_compact!("{prefix}name").as_str())?,
            url: row.try_get(compact_str::format_compact!("{prefix}url").as_str())?,
            secret: row.try_get(compact_str::format_compact!("{prefix}secret").as_str())?,
            events: row.try_get(compact_str::format_compact!("{prefix}events").as_str())?,
            enabled: row.try_get(compact_str::format_compact!("{prefix}enabled").as_str())?,
            created_by_uuid: row
                .try_get(compact_str::format_compact!("{prefix}created_by_uuid").as_str())?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            updated: row.try_get(compact_str::format_compact!("{prefix}updated").as_str())?,
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

impl WebhookSubscription {
    /// Generates a fresh high-entropy signing secret. The returned plaintext
    /// must be shown to the admin exactly once and never stored or logged.
    pub fn generate_secret() -> String {
        rand::distr::Alphanumeric.sample_string(&mut rand::rng(), 48)
    }

    pub async fn decrypt_secret(
        &self,
        database: &crate::database::Database,
    ) -> Result<compact_str::CompactString, anyhow::Error> {
        self.secret.decrypt(database).await
    }

    pub async fn all_with_pagination(
        database: &crate::database::Database,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        super::PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            format!(
                r#"
                FROM webhook_subscriptions
                WHERE {search}
                "#,
                search = super::search_sql(
                    1,
                    &["webhook_subscriptions.name", "webhook_subscriptions.url"],
                    &["webhook_subscriptions.uuid"],
                ),
            ),
            "webhook_subscriptions.created, webhook_subscriptions.uuid",
        )
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    /// Every enabled subscription interested in `event`, oldest first.
    pub async fn all_enabled_for_event(
        database: &crate::database::Database,
        event: &str,
    ) -> Result<Vec<Self>, crate::database::DatabaseError> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM webhook_subscriptions
            WHERE webhook_subscriptions.enabled = true AND $1 = ANY(webhook_subscriptions.events)
            ORDER BY webhook_subscriptions.created, webhook_subscriptions.uuid
            "#,
            Self::columns_sql(None)
        )))
        .bind(event)
        .fetch_all(database.read())
        .await?;

        rows.into_iter()
            .map(|row| Self::map(None, &row))
            .try_collect_vec()
    }

    /// Replaces the signing secret (rotation). The caller owns showing the
    /// new plaintext secret to the admin exactly once.
    pub async fn rotate_secret(
        &mut self,
        state: &crate::State,
        secret: EncryptedString,
    ) -> Result<(), crate::database::DatabaseError> {
        sqlx::query(
            r#"
            UPDATE webhook_subscriptions
            SET secret = $1, updated = NOW()
            WHERE webhook_subscriptions.uuid = $2
            "#,
        )
        .bind(&secret)
        .bind(self.uuid)
        .execute(state.database.write())
        .await?;

        self.secret = secret;
        self.updated = chrono::Utc::now().naive_utc();

        Ok(())
    }
}

#[async_trait::async_trait]
impl IntoAdminApiObject for WebhookSubscription {
    type AdminApiObject = AdminApiWebhookSubscription;
    type ExtraArgs<'a> = ();

    async fn into_admin_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        let api_object = AdminApiWebhookSubscription::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            AdminApiWebhookSubscription {
                uuid: self.uuid,
                name: self.name,
                url: self.url,
                events: self.events,
                enabled: self.enabled,
                // The secret itself is never serialized; admins only ever see
                // the plaintext once, in the create/rotate responses.
                has_secret: true,
                created_by_uuid: self.created_by_uuid,
                created: self.created.and_utc(),
                updated: self.updated.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}

#[async_trait::async_trait]
impl ByUuid for WebhookSubscription {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM webhook_subscriptions
            WHERE webhook_subscriptions.uuid = $1
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
            FROM webhook_subscriptions
            WHERE webhook_subscriptions.uuid = $1
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
pub struct CreateWebhookSubscriptionOptions {
    #[garde(length(chars, min = 1, max = 128))]
    #[schema(min_length = 1, max_length = 128)]
    pub name: compact_str::CompactString,
    #[garde(custom(validate_webhook_url))]
    #[schema(min_length = 1, max_length = 2048)]
    pub url: compact_str::CompactString,
    #[garde(skip)]
    pub secret: EncryptedString,
    #[garde(custom(validate_webhook_events))]
    pub events: Vec<String>,
    #[garde(skip)]
    pub enabled: bool,
    #[garde(skip)]
    pub created_by_uuid: Option<uuid::Uuid>,
}

#[async_trait::async_trait]
impl CreatableModel for WebhookSubscription {
    type CreateOptions<'a> = CreateWebhookSubscriptionOptions;
    type CreateResult = Self;

    fn get_create_handlers() -> &'static LazyLock<CreateListenerList<Self>> {
        static CREATE_LISTENERS: LazyLock<CreateListenerList<WebhookSubscription>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &CREATE_LISTENERS
    }

    async fn create_with_transaction(
        state: &crate::State,
        mut options: Self::CreateOptions<'_>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, crate::database::DatabaseError> {
        options.validate()?;

        let mut query_builder = InsertQueryBuilder::new("webhook_subscriptions");

        Self::run_create_handlers(&mut options, &mut query_builder, state, transaction).await?;

        let now = chrono::Utc::now().naive_utc();

        query_builder
            .set("name", &options.name)
            .set("url", &options.url)
            .set("secret", &options.secret)
            .set("events", &options.events)
            .set("enabled", options.enabled)
            .set("created_by_uuid", options.created_by_uuid)
            .set("created", now)
            .set("updated", now);

        let row = query_builder
            .returning(&Self::columns_sql(None))
            .fetch_one(&mut **transaction)
            .await?;
        let mut subscription = Self::map(None, &row)?;

        Self::run_after_create_handlers(&mut subscription, &options, state, transaction).await?;

        Ok(subscription)
    }
}

#[derive(ToSchema, Serialize, Deserialize, Validate, Clone, Default)]
pub struct UpdateWebhookSubscriptionOptions {
    #[garde(length(chars, min = 1, max = 128))]
    #[schema(min_length = 1, max_length = 128)]
    pub name: Option<compact_str::CompactString>,
    #[garde(inner(custom(validate_webhook_url)))]
    #[schema(min_length = 1, max_length = 2048)]
    pub url: Option<compact_str::CompactString>,
    #[garde(inner(custom(validate_webhook_events)))]
    pub events: Option<Vec<String>>,
    #[garde(skip)]
    pub enabled: Option<bool>,
}

#[async_trait::async_trait]
impl UpdatableModel for WebhookSubscription {
    type UpdateOptions = UpdateWebhookSubscriptionOptions;

    fn get_update_handlers() -> &'static LazyLock<UpdateHandlerList<Self>> {
        static UPDATE_LISTENERS: LazyLock<UpdateHandlerList<WebhookSubscription>> =
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

        let mut query_builder = UpdateQueryBuilder::new("webhook_subscriptions");

        self.run_update_handlers(&mut options, &mut query_builder, state, transaction)
            .await?;

        query_builder
            .set("name", options.name.as_ref())
            .set("url", options.url.as_ref())
            .set("events", options.events.as_ref())
            .set("enabled", options.enabled)
            .set("updated", Some(chrono::Utc::now().naive_utc()))
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
        self.updated = chrono::Utc::now().naive_utc();

        self.run_after_update_handlers(state, transaction).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl DeletableModel for WebhookSubscription {
    type DeleteOptions = ();

    fn get_delete_handlers() -> &'static LazyLock<DeleteHandlerList<Self>> {
        static DELETE_LISTENERS: LazyLock<DeleteHandlerList<WebhookSubscription>> =
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
            DELETE FROM webhook_subscriptions
            WHERE webhook_subscriptions.uuid = $1
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
#[init_args(WebhookSubscription, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "AdminWebhookSubscription")]
pub struct AdminApiWebhookSubscription {
    pub uuid: uuid::Uuid,

    pub name: compact_str::CompactString,
    pub url: compact_str::CompactString,
    pub events: Vec<String>,
    pub enabled: bool,

    /// Whether a signing secret is stored. The secret value itself is never
    /// returned by any read endpoint.
    pub has_secret: bool,

    pub created_by_uuid: Option<uuid::Uuid>,

    pub created: chrono::DateTime<chrono::Utc>,
    pub updated: chrono::DateTime<chrono::Utc>,
}
