use crate::{models::InsertQueryBuilder, prelude::*};
use garde::Validate;
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};
use utoipa::ToSchema;

/// One recorded attempt to deliver a webhook payload. Rows are append-only:
/// a subscription that is deleted takes its delivery log with it (the
/// `webhook_deliveries.subscription_uuid` foreign key is `ON DELETE CASCADE`).
#[derive(Serialize, Deserialize, Clone)]
pub struct WebhookDelivery {
    pub uuid: uuid::Uuid,
    pub subscription_uuid: uuid::Uuid,

    pub event: compact_str::CompactString,
    /// 1 for the first try, 2 for the single retry.
    pub attempt: i32,
    pub status_code: Option<i32>,
    pub success: bool,
    pub error: Option<compact_str::CompactString>,

    pub created: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for WebhookDelivery {
    const NAME: &'static str = "webhook_delivery";

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
                "webhook_deliveries.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "webhook_deliveries.subscription_uuid",
                compact_str::format_compact!("{prefix}subscription_uuid"),
            ),
            (
                "webhook_deliveries.event",
                compact_str::format_compact!("{prefix}event"),
            ),
            (
                "webhook_deliveries.attempt",
                compact_str::format_compact!("{prefix}attempt"),
            ),
            (
                "webhook_deliveries.status_code",
                compact_str::format_compact!("{prefix}status_code"),
            ),
            (
                "webhook_deliveries.success",
                compact_str::format_compact!("{prefix}success"),
            ),
            (
                "webhook_deliveries.error",
                compact_str::format_compact!("{prefix}error"),
            ),
            (
                "webhook_deliveries.created",
                compact_str::format_compact!("{prefix}created"),
            ),
        ])
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        Ok(Self {
            uuid: row.try_get(compact_str::format_compact!("{prefix}uuid").as_str())?,
            subscription_uuid: row
                .try_get(compact_str::format_compact!("{prefix}subscription_uuid").as_str())?,
            event: row.try_get(compact_str::format_compact!("{prefix}event").as_str())?,
            attempt: row.try_get(compact_str::format_compact!("{prefix}attempt").as_str())?,
            status_code: row
                .try_get(compact_str::format_compact!("{prefix}status_code").as_str())?,
            success: row.try_get(compact_str::format_compact!("{prefix}success").as_str())?,
            error: row.try_get(compact_str::format_compact!("{prefix}error").as_str())?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }
}

impl WebhookDelivery {
    pub async fn by_subscription_uuid_with_pagination(
        database: &crate::database::Database,
        subscription_uuid: uuid::Uuid,
        page: i64,
        per_page: i64,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        super::PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            r#"
            FROM webhook_deliveries
            WHERE webhook_deliveries.subscription_uuid = $1
            "#,
            "webhook_deliveries.created DESC, webhook_deliveries.ctid DESC",
        )
        .bind(subscription_uuid)
        .fetch(database, page, per_page)
        .await
    }

    pub async fn delete_older_than(
        database: &crate::database::Database,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, crate::database::DatabaseError> {
        let result = sqlx::query(
            r#"
            DELETE FROM webhook_deliveries
            WHERE webhook_deliveries.created < $1
            "#,
        )
        .bind(cutoff.naive_utc())
        .execute(database.write())
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn retain_latest_logs_per_subscription(
        database: &crate::database::Database,
        keep_count: i64,
    ) -> Result<u64, crate::database::DatabaseError> {
        let result = sqlx::query(
            r#"
            DELETE FROM webhook_deliveries
            WHERE ctid IN (
                SELECT ctid
                FROM (
                    SELECT
                        ctid,
                        ROW_NUMBER() OVER (PARTITION BY webhook_deliveries.subscription_uuid ORDER BY webhook_deliveries.created DESC, webhook_deliveries.ctid DESC) rn
                    FROM webhook_deliveries
                ) sub
                WHERE sub.rn > $1
            )
            "#,
        )
        .bind(keep_count)
        .execute(database.write())
        .await?;

        Ok(result.rows_affected())
    }
}

#[async_trait::async_trait]
impl IntoAdminApiObject for WebhookDelivery {
    type AdminApiObject = AdminApiWebhookDelivery;
    type ExtraArgs<'a> = ();

    async fn into_admin_api_object<'a>(
        self,
        _state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        Ok(AdminApiWebhookDelivery {
            uuid: self.uuid,
            subscription_uuid: self.subscription_uuid,
            event: self.event,
            attempt: self.attempt,
            status_code: self.status_code,
            success: self.success,
            error: self.error,
            created: self.created.and_utc(),
        })
    }
}

#[derive(ToSchema, Deserialize, Validate)]
pub struct CreateWebhookDeliveryOptions {
    #[garde(skip)]
    pub subscription_uuid: uuid::Uuid,
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub event: compact_str::CompactString,
    #[garde(skip)]
    pub attempt: i32,
    #[garde(skip)]
    pub status_code: Option<i32>,
    #[garde(skip)]
    pub success: bool,
    #[garde(skip)]
    pub error: Option<compact_str::CompactString>,
    #[garde(skip)]
    pub created: Option<chrono::NaiveDateTime>,
}

#[async_trait::async_trait]
impl CreatableModel for WebhookDelivery {
    type CreateOptions<'a> = CreateWebhookDeliveryOptions;
    type CreateResult = ();

    fn get_create_handlers() -> &'static LazyLock<CreateListenerList<Self>> {
        static CREATE_LISTENERS: LazyLock<CreateListenerList<WebhookDelivery>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &CREATE_LISTENERS
    }

    async fn create_with_transaction(
        state: &crate::State,
        mut options: Self::CreateOptions<'_>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self::CreateResult, crate::database::DatabaseError> {
        options.validate()?;

        let mut query_builder = InsertQueryBuilder::new("webhook_deliveries");

        Self::run_create_handlers(&mut options, &mut query_builder, state, transaction).await?;

        query_builder
            .set("subscription_uuid", options.subscription_uuid)
            .set("event", &options.event)
            .set("attempt", options.attempt)
            .set("status_code", options.status_code)
            .set("success", options.success)
            .set("error", options.error.as_ref());

        if let Some(created) = options.created {
            query_builder.set("created", created);
        }

        query_builder.execute(&mut **transaction).await?;

        let mut result = ();

        Self::run_after_create_handlers(&mut result, &options, state, transaction).await?;

        Ok(result)
    }
}

#[derive(ToSchema, Serialize)]
#[schema(title = "AdminWebhookDelivery")]
pub struct AdminApiWebhookDelivery {
    pub uuid: uuid::Uuid,
    pub subscription_uuid: uuid::Uuid,

    pub event: compact_str::CompactString,
    pub attempt: i32,
    pub status_code: Option<i32>,
    pub success: bool,
    pub error: Option<compact_str::CompactString>,

    pub created: chrono::DateTime<chrono::Utc>,
}
