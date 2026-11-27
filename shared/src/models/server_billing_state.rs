use crate::{
    models::UpdateQueryBuilder,
    prelude::*,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::{
    collections::BTreeMap,
    sync::LazyLock,
};

/// Per-server billing bookkeeping. This is NOT the server itself — it
/// only records when a server was last charged, so the hourly task is
/// idempotent-ish: a server is charged once per hour, and a task restart
/// or a second run inside the same hour does not charge it twice.
///
/// `last_charged_at` is `None` until the first charge. The task sets it
/// to the hour it charged for (truncated to the hour), not to "now", so
/// a run at 10:07 and a retry at 10:40 both agree the 10:00 hour is done.
#[derive(Serialize, Deserialize, Clone)]
pub struct ServerBillingState {
    pub uuid: uuid::Uuid,
    pub server_uuid: uuid::Uuid,
    pub last_charged_at: Option<chrono::NaiveDateTime>,
    pub total_charged_credits: i64,
    pub created: chrono::NaiveDateTime,
    pub updated: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for ServerBillingState {
    const NAME: &'static str = "server_billing_state";

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
                "server_billing_states.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "server_billing_states.server_uuid",
                compact_str::format_compact!("{prefix}server_uuid"),
            ),
            (
                "server_billing_states.last_charged_at",
                compact_str::format_compact!("{prefix}last_charged_at"),
            ),
            (
                "server_billing_states.total_charged_credits",
                compact_str::format_compact!("{prefix}total_charged_credits"),
            ),
            (
                "server_billing_states.created",
                compact_str::format_compact!("{prefix}created"),
            ),
            (
                "server_billing_states.updated",
                compact_str::format_compact!("{prefix}updated"),
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
            last_charged_at: row
                .try_get(compact_str::format_compact!("{prefix}last_charged_at").as_str())?,
            total_charged_credits: row.try_get(
                compact_str::format_compact!("{prefix}total_charged_credits").as_str(),
            )?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            updated: row.try_get(compact_str::format_compact!("{prefix}updated").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }

    fn cache_invalidation_keys(&self) -> Vec<compact_str::CompactString> {
        vec![
            compact_str::format_compact!("{}::{}", Self::NAME, self.uuid),
            compact_str::format_compact!("{}::{}", Self::NAME, self.server_uuid),
        ]
    }
}

impl ServerBillingState {
    pub async fn by_server_uuid(
        database: &crate::database::Database,
        server_uuid: uuid::Uuid,
    ) -> Result<Option<Self>, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_billing_states
            WHERE server_billing_states.server_uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(server_uuid)
        .fetch_optional(database.read())
        .await?;

        row.try_map(|row| Self::map(None, &row))
    }

    /// Fetch-or-create the state row inside the caller's transaction,
    /// locking it, so two hourly runs cannot charge the same server twice.
    pub async fn get_or_create_locked(
        server_uuid: uuid::Uuid,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            INSERT INTO server_billing_states (server_uuid, total_charged_credits)
            VALUES ($1, 0)
            ON CONFLICT (server_uuid) DO NOTHING
            RETURNING {}
            "#,
            Self::columns_sql(None)
        )))
        .bind(server_uuid)
        .fetch_optional(&mut **transaction)
        .await?;

        if let Some(row) = row {
            return Self::map(None, &row);
        }

        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_billing_states
            WHERE server_billing_states.server_uuid = $1
            FOR UPDATE
            "#,
            Self::columns_sql(None)
        )))
        .bind(server_uuid)
        .fetch_one(&mut **transaction)
        .await?;

        Self::map(None, &row)
    }

    /// Record that the hour starting at `hour` was charged for
    /// `charged_credits`. Callers must already have applied the charge
    /// to the owner's balance in the same transaction.
    pub async fn mark_charged(
        &mut self,
        hour: chrono::NaiveDateTime,
        charged_credits: i64,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), crate::database::DatabaseError> {
        let total = self
            .total_charged_credits
            .checked_add(charged_credits)
            .ok_or_else(|| anyhow::anyhow!("total charged credits overflow"))?;

        let mut query_builder = UpdateQueryBuilder::new("server_billing_states");
        query_builder
            .set("last_charged_at", Some(hour))
            .set("total_charged_credits", Some(total))
            .where_eq("uuid", self.uuid);
        query_builder.execute(&mut **transaction).await?;

        self.last_charged_at = Some(hour);
        self.total_charged_credits = total;
        self.updated = chrono::Utc::now().naive_utc();

        Ok(())
    }
}

#[async_trait::async_trait]
impl ByUuid for ServerBillingState {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_billing_states
            WHERE server_billing_states.uuid = $1
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
            FROM server_billing_states
            WHERE server_billing_states.uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(uuid)
        .fetch_one(&mut **transaction)
        .await?;

        Self::map(None, &row)
    }
}

#[schema_extension_derive::extendible]
#[init_args(ServerBillingState, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "AdminServerBillingState")]
pub struct AdminApiServerBillingState {
    pub uuid: uuid::Uuid,
    pub server_uuid: uuid::Uuid,
    pub last_charged_at: Option<chrono::DateTime<chrono::Utc>>,
    pub total_charged_credits: i64,
    pub created: chrono::DateTime<chrono::Utc>,
    pub updated: chrono::DateTime<chrono::Utc>,
}

#[async_trait::async_trait]
impl IntoAdminApiObject for ServerBillingState {
    type AdminApiObject = AdminApiServerBillingState;
    type ExtraArgs<'a> = ();

    async fn into_admin_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        let api_object = AdminApiServerBillingState::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            AdminApiServerBillingState {
                uuid: self.uuid,
                server_uuid: self.server_uuid,
                last_charged_at: self.last_charged_at.map(|dt| dt.and_utc()),
                total_charged_credits: self.total_charged_credits,
                created: self.created.and_utc(),
                updated: self.updated.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}
