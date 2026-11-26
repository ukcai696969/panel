use crate::{
    models::{
        InsertQueryBuilder, UpdateQueryBuilder,
        user_credit_transaction::{
            CreateUserCreditTransactionOptions, UserCreditTransaction,
            UserCreditTransactionKind,
        },
    },
    prelude::*,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};
use utoipa::ToSchema;

/// One user's credit balance. 1 credit = 1 minor currency unit (1 cent
/// for USD/GBP/EUR) — see `shared::settings::billing`.
///
/// The balance is the source of truth; the transaction ledger
/// (`UserCreditTransaction`) is the audit trail. Every balance change
/// MUST go through [`UserCredit::apply`] (or [`UserCredit::apply_with_transaction`]),
/// which updates the balance and writes the ledger row in the same
/// database transaction, holding a row lock so two concurrent charges
/// cannot both read the same balance.
#[derive(Serialize, Deserialize, Clone)]
pub struct UserCredit {
    pub uuid: uuid::Uuid,
    pub user_uuid: uuid::Uuid,
    pub balance_credits: i64,
    pub created: chrono::NaiveDateTime,
    pub updated: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for UserCredit {
    const NAME: &'static str = "user_credit";

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
                "user_credits.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "user_credits.user_uuid",
                compact_str::format_compact!("{prefix}user_uuid"),
            ),
            (
                "user_credits.balance_credits",
                compact_str::format_compact!("{prefix}balance_credits"),
            ),
            (
                "user_credits.created",
                compact_str::format_compact!("{prefix}created"),
            ),
            (
                "user_credits.updated",
                compact_str::format_compact!("{prefix}updated"),
            ),
        ])
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        Ok(Self {
            uuid: row.try_get(compact_str::format_compact!("{prefix}uuid").as_str())?,
            user_uuid: row.try_get(compact_str::format_compact!("{prefix}user_uuid").as_str())?,
            balance_credits: row
                .try_get(compact_str::format_compact!("{prefix}balance_credits").as_str())?,
            created: row.try_get(compact_str::format_compact!("{prefix}created").as_str())?,
            updated: row.try_get(compact_str::format_compact!("{prefix}updated").as_str())?,
            extension_data: Self::map_extensions(prefix, row)?,
        })
    }

    fn cache_invalidation_keys(&self) -> Vec<compact_str::CompactString> {
        vec![
            compact_str::format_compact!("{}::{}", Self::NAME, self.uuid),
            compact_str::format_compact!("{}::{}", Self::NAME, self.user_uuid),
        ]
    }
}

impl UserCredit {
    pub async fn by_user_uuid(
        database: &crate::database::Database,
        user_uuid: uuid::Uuid,
    ) -> Result<Option<Self>, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM user_credits
            WHERE user_credits.user_uuid = $1
            "#,
            Self::columns_sql(None)
        )))
        .bind(user_uuid)
        .fetch_optional(database.read())
        .await?;

        row.try_map(|row| Self::map(None, &row))
    }

    /// Balance for a user, creating a zero row on first use. The insert
    /// is `ON CONFLICT DO NOTHING` + re-read, so two concurrent first
    /// uses cannot create two rows.
    pub async fn get_or_create_with_transaction(
        state: &crate::State,
        user_uuid: uuid::Uuid,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            INSERT INTO user_credits (user_uuid, balance_credits)
            VALUES ($1, 0)
            ON CONFLICT (user_uuid) DO NOTHING
            RETURNING {}
            "#,
            Self::columns_sql(None)
        )))
        .bind(user_uuid)
        .fetch_optional(&mut **transaction)
        .await?;

        if let Some(row) = row {
            return Self::map(None, &row);
        }

        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM user_credits
            WHERE user_credits.user_uuid = $1
            FOR UPDATE
            "#,
            Self::columns_sql(None)
        )))
        .bind(user_uuid)
        .fetch_one(&mut **transaction)
        .await?;

        Self::map(None, &row)
    }

    /// Apply one signed movement and write its ledger row, atomically.
    /// `amount_credits` must not be 0. The new balance may go negative
    /// (charges do that); suspension is decided by the caller / hourly
    /// task, not here, so a single failed charge never loses the ledger.
    pub async fn apply_with_transaction(
        state: &crate::State,
        user_uuid: uuid::Uuid,
        amount_credits: i64,
        kind: UserCreditTransactionKind,
        server_uuid: Option<uuid::Uuid>,
        reference: Option<compact_str::CompactString>,
        description: Option<compact_str::CompactString>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(Self, UserCreditTransaction), crate::database::DatabaseError> {
        if amount_credits == 0 {
            return Err(anyhow::anyhow!("credit movement amount must not be zero").into());
        }

        let mut credit = Self::get_or_create_with_transaction(state, user_uuid, transaction).await?;
        let new_balance = credit
            .balance_credits
            .checked_add(amount_credits)
            .ok_or_else(|| anyhow::anyhow!("credit balance overflow"))?;

        let mut query_builder = UpdateQueryBuilder::new("user_credits");
        query_builder
            .set("balance_credits", Some(new_balance))
            .where_eq("uuid", credit.uuid);
        query_builder.execute(&mut **transaction).await?;

        credit.balance_credits = new_balance;
        credit.updated = chrono::Utc::now().naive_utc();

        let ledger = UserCreditTransaction::create_with_transaction(
            state,
            CreateUserCreditTransactionOptions {
                user_uuid,
                kind,
                amount_credits,
                balance_after_credits: new_balance,
                server_uuid,
                reference,
                description,
            },
            transaction,
        )
        .await?;

        Ok((credit, ledger))
    }

    /// Convenience wrapper that owns its transaction.
    pub async fn apply(
        state: &crate::State,
        user_uuid: uuid::Uuid,
        amount_credits: i64,
        kind: UserCreditTransactionKind,
        server_uuid: Option<uuid::Uuid>,
        reference: Option<compact_str::CompactString>,
        description: Option<compact_str::CompactString>,
    ) -> Result<(Self, UserCreditTransaction), crate::database::DatabaseError> {
        let mut transaction = state.database.write().begin().await?;

        let result = match Self::apply_with_transaction(
            state,
            user_uuid,
            amount_credits,
            kind,
            server_uuid,
            reference,
            description,
            &mut transaction,
        )
        .await
        {
            Ok(result) => result,
            Err(err) => {
                transaction.rollback().await?;
                return Err(err);
            }
        };

        transaction.commit().await?;

        super::invalidate_cache_keys(
            &state.database,
            &result.0.cache_invalidation_keys(),
        )
        .await;

        Ok(result)
    }

    /// All balances, newest activity first, for the admin balances page.
    pub async fn all_with_pagination(
        database: &crate::database::Database,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        // Search is by user uuid / username / email via a join, so this
        // query is spelled out instead of using search_sql on this table.
        super::PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            format!(
                r#"
                FROM user_credits
                JOIN users ON users.uuid = user_credits.user_uuid
                WHERE ($1::text IS NULL
                    OR users.username ILIKE '%' || $1 || '%'
                    OR users.email ILIKE '%' || $1 || '%'
                    OR user_credits.user_uuid::text LIKE lower($1) || '%')
                "#,
            ),
            "user_credits.updated DESC, user_credits.uuid DESC",
        )
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }
}

#[async_trait::async_trait]
impl ByUuid for UserCredit {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM user_credits
            WHERE user_credits.uuid = $1
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
            FROM user_credits
            WHERE user_credits.uuid = $1
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
#[init_args(UserCredit, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "AdminUserCredit")]
pub struct AdminApiUserCredit {
    pub uuid: uuid::Uuid,
    pub user_uuid: uuid::Uuid,
    pub balance_credits: i64,
    pub created: chrono::DateTime<chrono::Utc>,
    pub updated: chrono::DateTime<chrono::Utc>,
}

#[schema_extension_derive::extendible]
#[init_args(UserCredit, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "ApiUserCredit")]
pub struct ApiUserCredit {
    pub balance_credits: i64,
    pub updated: chrono::DateTime<chrono::Utc>,
}

#[async_trait::async_trait]
impl IntoAdminApiObject for UserCredit {
    type AdminApiObject = AdminApiUserCredit;
    type ExtraArgs<'a> = ();

    async fn into_admin_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        let api_object = AdminApiUserCredit::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            AdminApiUserCredit {
                uuid: self.uuid,
                user_uuid: self.user_uuid,
                balance_credits: self.balance_credits,
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
impl IntoApiObject for UserCredit {
    type ApiObject = ApiUserCredit;
    type ExtraArgs<'a> = ();

    async fn into_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::ApiObject, crate::database::DatabaseError> {
        let api_object = ApiUserCredit::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            ApiUserCredit {
                balance_credits: self.balance_credits,
                updated: self.updated.and_utc(),
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}
