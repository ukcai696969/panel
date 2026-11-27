use crate::{
    models::{InsertQueryBuilder, PaginatedQueryBuilder},
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

/// What a credit movement was. Stored as TEXT in the database (see the
/// billing migration) and parsed here, so adding a kind needs no new
/// Postgres enum — only a new variant + a migration CHECK update.
#[derive(
    ToSchema, Serialize, Deserialize, Type, PartialEq, Eq, Hash, Clone, Copy, Debug,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum UserCreditTransactionKind {
    /// Credits bought, credited by a verified Stripe webhook.
    Purchase,
    /// Hourly server charge (negative amount).
    Charge,
    /// Credits returned, e.g. after a refunded purchase (negative amount
    /// when it takes credits away, positive when it restores a charge).
    Refund,
    /// Manual admin grant / correction, either sign.
    Adjustment,
}

impl UserCreditTransactionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Purchase => "purchase",
            Self::Charge => "charge",
            Self::Refund => "refund",
            Self::Adjustment => "adjustment",
        }
    }
}

impl std::str::FromStr for UserCreditTransactionKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "purchase" => Ok(Self::Purchase),
            "charge" => Ok(Self::Charge),
            "refund" => Ok(Self::Refund),
            "adjustment" => Ok(Self::Adjustment),
            other => Err(anyhow::anyhow!("unknown credit transaction kind: {other}")),
        }
    }
}

/// One immutable movement of credits for one user.
///
/// `amount_credits` is signed: purchases and positive adjustments add,
/// charges subtract. `balance_after_credits` is the user's balance
/// immediately after this row was applied, so the ledger can be audited
/// without replaying anything. Rows are never updated or deleted.
#[derive(Serialize, Deserialize, Clone)]
pub struct UserCreditTransaction {
    pub uuid: uuid::Uuid,
    pub user_uuid: uuid::Uuid,
    pub kind: UserCreditTransactionKind,
    pub amount_credits: i64,
    pub balance_after_credits: i64,
    /// Server the charge relates to, if any.
    pub server_uuid: Option<uuid::Uuid>,
    /// Stripe checkout session id for purchases, or a free-form admin
    /// note / idempotency reference for other kinds.
    pub reference: Option<compact_str::CompactString>,
    pub description: Option<compact_str::CompactString>,
    pub created: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for UserCreditTransaction {
    const NAME: &'static str = "user_credit_transaction";

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
                "user_credit_transactions.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "user_credit_transactions.user_uuid",
                compact_str::format_compact!("{prefix}user_uuid"),
            ),
            (
                "user_credit_transactions.kind",
                compact_str::format_compact!("{prefix}kind"),
            ),
            (
                "user_credit_transactions.amount_credits",
                compact_str::format_compact!("{prefix}amount_credits"),
            ),
            (
                "user_credit_transactions.balance_after_credits",
                compact_str::format_compact!("{prefix}balance_after_credits"),
            ),
            (
                "user_credit_transactions.server_uuid",
                compact_str::format_compact!("{prefix}server_uuid"),
            ),
            (
                "user_credit_transactions.reference",
                compact_str::format_compact!("{prefix}reference"),
            ),
            (
                "user_credit_transactions.description",
                compact_str::format_compact!("{prefix}description"),
            ),
            (
                "user_credit_transactions.created",
                compact_str::format_compact!("{prefix}created"),
            ),
        ])
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        let kind_raw: String =
            row.try_get(compact_str::format_compact!("{prefix}kind").as_str())?;

        Ok(Self {
            uuid: row.try_get(compact_str::format_compact!("{prefix}uuid").as_str())?,
            user_uuid: row.try_get(compact_str::format_compact!("{prefix}user_uuid").as_str())?,
            kind: kind_raw.parse()?,
            amount_credits: row
                .try_get(compact_str::format_compact!("{prefix}amount_credits").as_str())?,
            balance_after_credits: row.try_get(
                compact_str::format_compact!("{prefix}balance_after_credits").as_str(),
            )?,
            server_uuid: row
                .try_get(compact_str::format_compact!("{prefix}server_uuid").as_str())?,
            reference: row
                .try_get(compact_str::format_compact!("{prefix}reference").as_str())?,
            description: row
                .try_get(compact_str::format_compact!("{prefix}description").as_str())?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }

    fn cache_invalidation_keys(&self) -> Vec<compact_str::CompactString> {
        vec![
            compact_str::format_compact!("{}::{}", Self::NAME, self.uuid),
            compact_str::format_compact!("user_credit::{}", self.user_uuid),
        ]
    }
}

impl UserCreditTransaction {
    pub async fn by_user_uuid_with_pagination(
        database: &crate::database::Database,
        user_uuid: uuid::Uuid,
        page: i64,
        per_page: i64,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            "FROM user_credit_transactions WHERE user_credit_transactions.user_uuid = $1",
            "user_credit_transactions.created DESC, user_credit_transactions.uuid DESC",
        )
        .bind(user_uuid)
        .fetch(database, page, per_page)
        .await
    }

    pub async fn all_with_pagination(
        database: &crate::database::Database,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            format!(
                r#"
                FROM user_credit_transactions
                WHERE {search}
                "#,
                search = super::search_sql(
                    1,
                    &["user_credit_transactions.reference", "user_credit_transactions.description"],
                    &[
                        "user_credit_transactions.uuid",
                        "user_credit_transactions.user_uuid",
                        "user_credit_transactions.server_uuid",
                    ],
                ),
            ),
            "user_credit_transactions.created DESC, user_credit_transactions.uuid DESC",
        )
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    /// True when a purchase with this Stripe reference was already
    /// credited. The webhook must check this before crediting, so a
    /// retried / replayed Stripe event can never double-credit a user.
    pub async fn exists_by_reference(
        database: &crate::database::Database,
        reference: &str,
    ) -> Result<bool, crate::database::DatabaseError> {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM user_credit_transactions
                WHERE user_credit_transactions.reference = $1
                    AND user_credit_transactions.kind = 'purchase'
            )
            "#,
        )
        .bind(reference)
        .fetch_one(database.read())
        .await?;

        Ok(exists)
    }
}

#[async_trait::async_trait]
impl ByUuid for UserCreditTransaction {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM user_credit_transactions
            WHERE user_credit_transactions.uuid = $1
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
            FROM user_credit_transactions
            WHERE user_credit_transactions.uuid = $1
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
pub struct CreateUserCreditTransactionOptions {
    #[garde(skip)]
    pub user_uuid: uuid::Uuid,
    #[garde(skip)]
    pub kind: UserCreditTransactionKind,
    #[garde(skip)]
    pub amount_credits: i64,
    #[garde(skip)]
    pub balance_after_credits: i64,
    #[garde(skip)]
    pub server_uuid: Option<uuid::Uuid>,
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub reference: Option<compact_str::CompactString>,
    #[garde(length(chars, min = 1, max = 512))]
    #[schema(min_length = 1, max_length = 512)]
    pub description: Option<compact_str::CompactString>,
}

#[async_trait::async_trait]
impl CreatableModel for UserCreditTransaction {
    type CreateOptions<'a> = CreateUserCreditTransactionOptions;
    type CreateResult = Self;

    fn get_create_handlers() -> &'static LazyLock<CreateListenerList<Self>> {
        static CREATE_LISTENERS: LazyLock<CreateListenerList<UserCreditTransaction>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &CREATE_LISTENERS
    }

    async fn create_with_transaction(
        state: &crate::State,
        mut options: Self::CreateOptions<'_>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, crate::database::DatabaseError> {
        options.validate()?;

        let mut query_builder = InsertQueryBuilder::new("user_credit_transactions");

        Self::run_create_handlers(&mut options, &mut query_builder, state, transaction).await?;

        query_builder
            .set("user_uuid", options.user_uuid)
            .set("kind", options.kind.as_str())
            .set("amount_credits", options.amount_credits)
            .set("balance_after_credits", options.balance_after_credits)
            .set("server_uuid", options.server_uuid)
            .set("reference", options.reference.as_deref())
            .set("description", options.description.as_deref());

        let row = query_builder
            .returning(&Self::columns_sql(None))
            .fetch_one(&mut **transaction)
            .await?;
        let mut result = Self::map(None, &row)?;

        Self::run_after_create_handlers(&mut result, &options, state, transaction).await?;

        Ok(result)
    }
}

#[schema_extension_derive::extendible]
#[init_args(UserCreditTransaction, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "AdminUserCreditTransaction")]
pub struct AdminApiUserCreditTransaction {
    pub uuid: uuid::Uuid,
    pub user_uuid: uuid::Uuid,
    pub kind: UserCreditTransactionKind,
    pub amount_credits: i64,
    pub balance_after_credits: i64,
    pub server_uuid: Option<uuid::Uuid>,
    pub reference: Option<compact_str::CompactString>,
    pub description: Option<compact_str::CompactString>,
    pub created: chrono::DateTime<chrono::Utc>,
}

#[schema_extension_derive::extendible]
#[init_args(UserCreditTransaction, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "ApiUserCreditTransaction")]
pub struct ApiUserCreditTransaction {
    pub uuid: uuid::Uuid,
    pub kind: UserCreditTransactionKind,
    pub amount_credits: i64,
    pub balance_after_credits: i64,
    pub server_uuid: Option<uuid::Uuid>,
    pub description: Option<compact_str::CompactString>,
    pub created: chrono::DateTime<chrono::Utc>,
}

#[async_trait::async_trait]
impl IntoAdminApiObject for UserCreditTransaction {
    type AdminApiObject = AdminApiUserCreditTransaction;
    type ExtraArgs<'a> = ();

    async fn into_admin_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        let api_object = AdminApiUserCreditTransaction::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            AdminApiUserCreditTransaction {
                uuid: self.uuid,
                user_uuid: self.user_uuid,
                kind: self.kind,
                amount_credits: self.amount_credits,
                balance_after_credits: self.balance_after_credits,
                server_uuid: self.server_uuid,
                reference: self.reference,
                description: self.description,
                created: self.created.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}

#[async_trait::async_trait]
impl IntoApiObject for UserCreditTransaction {
    type ApiObject = ApiUserCreditTransaction;
    type ExtraArgs<'a> = ();

    async fn into_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::ApiObject, crate::database::DatabaseError> {
        let api_object = ApiUserCreditTransaction::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            ApiUserCreditTransaction {
                uuid: self.uuid,
                kind: self.kind,
                amount_credits: self.amount_credits,
                balance_after_credits: self.balance_after_credits,
                server_uuid: self.server_uuid,
                description: self.description,
                created: self.created.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}
