use crate::{
    models::{
        InsertQueryBuilder, UpdateQueryBuilder,
        server::{AdminApiServerLimits, ApiServerFeatureLimits, TrashSettings},
        server_schedule::ExportedServerSchedule,
        server_variable::ServerVariable,
    },
    prelude::*,
};
use garde::Validate;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};
use utoipa::ToSchema;

/// The portable snapshot stored on a template: everything needed to create a
/// server from it, without pointing at the (possibly later deleted) source
/// server. Variables are keyed by env variable name, not variable uuid, so a
/// template survives an egg being re-imported with new variable uuids.
#[derive(ToSchema, Validate, Serialize, Deserialize, Clone)]
pub struct ServerTemplateSnapshot {
    #[garde(skip)]
    pub egg_uuid: uuid::Uuid,
    #[garde(skip)]
    pub backup_configuration_uuid: Option<uuid::Uuid>,

    #[garde(skip)]
    pub variables: IndexMap<compact_str::CompactString, compact_str::CompactString>,

    #[garde(dive)]
    pub limits: AdminApiServerLimits,
    #[garde(skip)]
    pub pinned_cpus: Vec<i16>,
    #[garde(dive)]
    pub feature_limits: ApiServerFeatureLimits,

    #[garde(length(chars, min = 1, max = 8192))]
    #[schema(min_length = 1, max_length = 8192)]
    pub startup: compact_str::CompactString,
    #[garde(length(chars, min = 2, max = 255))]
    #[schema(min_length = 2, max_length = 255)]
    pub image: compact_str::CompactString,
    #[garde(skip)]
    pub labels: IndexMap<compact_str::CompactString, compact_str::CompactString>,
    #[garde(skip)]
    pub timezone: Option<compact_str::CompactString>,
    #[garde(dive)]
    #[schema(inline)]
    pub trash: Option<TrashSettings>,

    #[garde(skip)]
    pub hugepages_passthrough_enabled: bool,
    #[garde(skip)]
    pub kvm_passthrough_enabled: bool,

    /// How many allocations the source server had (primary + additional).
    /// A server created from the template gets this many *fresh* allocations.
    #[garde(range(min = 0))]
    #[schema(minimum = 0)]
    pub allocation_count: i32,

    #[garde(dive)]
    pub schedules: Vec<ExportedServerSchedule>,

    #[garde(skip)]
    pub source_server_uuid: Option<uuid::Uuid>,
}

impl ServerTemplateSnapshot {
    /// Snapshots a live server. Schedules are exported (not referenced), so
    /// deleting or editing the source server never changes the template.
    pub async fn from_server(
        database: &crate::database::Database,
        server: &super::server::Server,
    ) -> Result<Self, crate::database::DatabaseError> {
        let variables = ServerVariable::all_by_server_uuid_egg_uuid(
            database,
            server.uuid,
            server.egg.uuid,
        )
        .await?
        .into_iter()
        .map(|variable| (variable.variable.env_variable, variable.value.into()))
        .collect::<IndexMap<_, _>>();

        let allocation_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM server_allocations
            WHERE server_allocations.server_uuid = $1
            "#,
        )
        .bind(server.uuid)
        .fetch_one(database.read())
        .await? as i32;

        let mut schedules = Vec::new();
        let mut page = 1;
        loop {
            let page_result =
                super::server_schedule::ServerSchedule::by_server_uuid_with_pagination(
                    database,
                    server.uuid,
                    page,
                    100,
                    None,
                )
                .await?;

            let done = (page_result.data.len() as i64) < page_result.per_page;
            for schedule in page_result.data {
                schedules.push(schedule.into_exported(database).await?);
            }

            if done {
                break;
            }
            page += 1;
        }

        Ok(Self {
            egg_uuid: server.egg.uuid,
            backup_configuration_uuid: server.backup_configuration.map(|fetchable| fetchable.uuid),
            variables,
            limits: AdminApiServerLimits {
                cpu: server.cpu,
                memory: server.memory,
                memory_overhead: server.memory_overhead,
                swap: server.swap,
                disk: server.disk,
                io_weight: server.io_weight,
                bandwidth: Some(super::server::ApiServerBandwidth {
                    upload: server.bandwidth_upload as u64,
                    download: server.bandwidth_download as u64,
                }),
            },
            pinned_cpus: server.pinned_cpus.clone(),
            feature_limits: ApiServerFeatureLimits {
                allocations: server.allocation_limit,
                databases: server.database_limit,
                backups: server.backup_limit,
                schedules: server.schedule_limit,
                // The extendible `__overlay` carries extension data; a
                // template snapshot only stores the four numeric limits, so
                // it is empty here (no extension hooks run for this value).
                __overlay: Default::default(),
            },
            startup: server.startup.clone(),
            image: server.image.clone(),
            labels: server.labels.clone(),
            timezone: server.timezone.clone(),
            trash: server.trash,
            hugepages_passthrough_enabled: server.hugepages_passthrough_enabled,
            kvm_passthrough_enabled: server.kvm_passthrough_enabled,
            allocation_count,
            schedules,
            source_server_uuid: Some(server.uuid),
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ServerTemplate {
    pub uuid: uuid::Uuid,

    pub name: compact_str::CompactString,
    pub description: Option<compact_str::CompactString>,

    /// `None` is a global template, visible to every user and manageable only
    /// by admins. `Some(owner)` is a personal template, visible only to its
    /// owner (and to admins).
    pub owner_uuid: Option<uuid::Uuid>,
    pub owner: Option<super::user::User>,

    pub source_server_uuid: Option<uuid::Uuid>,
    pub egg: Box<super::nest_egg::NestEgg>,

    pub snapshot: ServerTemplateSnapshot,

    pub created: chrono::NaiveDateTime,
    pub updated: chrono::NaiveDateTime,

    extension_data: super::ModelExtensionData,
}

impl BaseModel for ServerTemplate {
    const NAME: &'static str = "server_template";

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

        let mut columns = BTreeMap::from([
            (
                "server_templates.uuid",
                compact_str::format_compact!("{prefix}uuid"),
            ),
            (
                "server_templates.name",
                compact_str::format_compact!("{prefix}name"),
            ),
            (
                "server_templates.description",
                compact_str::format_compact!("{prefix}description"),
            ),
            (
                "server_templates.owner_uuid",
                compact_str::format_compact!("{prefix}owner_uuid"),
            ),
            (
                "server_templates.source_server_uuid",
                compact_str::format_compact!("{prefix}source_server_uuid"),
            ),
            (
                "server_templates.snapshot",
                compact_str::format_compact!("{prefix}snapshot"),
            ),
            (
                "server_templates.created",
                compact_str::format_compact!("{prefix}created"),
            ),
            (
                "server_templates.updated",
                compact_str::format_compact!("{prefix}updated"),
            ),
        ]);

        columns.extend(super::user::User::base_columns(Some("owner_")));
        columns.extend(super::nest_egg::NestEgg::base_columns(Some("egg_")));

        columns
    }

    #[inline]
    fn map(prefix: Option<&str>, row: &PgRow) -> Result<Self, crate::database::DatabaseError> {
        let prefix = prefix.unwrap_or_default();

        Ok(Self {
            uuid: row.try_get(compact_str::format_compact!("{prefix}uuid").as_str())?,
            name: row.try_get(compact_str::format_compact!("{prefix}name").as_str())?,
            description: row
                .try_get(compact_str::format_compact!("{prefix}description").as_str())?,
            owner_uuid: row
                .try_get(compact_str::format_compact!("{prefix}owner_uuid").as_str())?,
            owner: match row.try_get::<uuid::Uuid, _>(
                compact_str::format_compact!("{prefix}owner_uuid").as_str(),
            ) {
                Ok(_) => Some(super::user::User::map(Some("owner_"), row)?),
                Err(_) => None,
            },
            source_server_uuid: row.try_get(
                compact_str::format_compact!("{prefix}source_server_uuid").as_str(),
            )?,
            egg: Box::new(super::nest_egg::NestEgg::map(Some("egg_"), row)?),
            snapshot: row
                .try_get::<sqlx::types::Json<_>, _>(
                    compact_str::format_compact!("{prefix}snapshot").as_str(),
                )?
                .0,
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

impl ServerTemplate {
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
                FROM server_templates
                LEFT JOIN users ON users.uuid = server_templates.owner_uuid
                LEFT JOIN roles ON roles.uuid = users.role_uuid
                JOIN nest_eggs ON nest_eggs.uuid = (server_templates.snapshot->>'egg_uuid')::uuid
                WHERE {search}
                "#,
                search = super::search_sql(
                    1,
                    &["server_templates.name"],
                    &["server_templates.uuid"]
                ),
            ),
            "server_templates.created, server_templates.uuid",
        )
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    /// Global templates plus the given user's own templates.
    pub async fn visible_by_user_uuid_with_pagination(
        database: &crate::database::Database,
        user_uuid: uuid::Uuid,
        page: i64,
        per_page: i64,
        search: Option<&str>,
    ) -> Result<super::Pagination<Self>, crate::database::DatabaseError> {
        super::PaginatedQueryBuilder::new(
            Self::columns_sql(None),
            format!(
                r#"
                FROM server_templates
                LEFT JOIN users ON users.uuid = server_templates.owner_uuid
                LEFT JOIN roles ON roles.uuid = users.role_uuid
                JOIN nest_eggs ON nest_eggs.uuid = (server_templates.snapshot->>'egg_uuid')::uuid
                WHERE (server_templates.owner_uuid IS NULL OR server_templates.owner_uuid = $2)
                    AND ({search})
                "#,
                search = super::search_sql(
                    3,
                    &["server_templates.name"],
                    &["server_templates.uuid"]
                ),
            ),
            "server_templates.created, server_templates.uuid",
        )
        .bind(search)
        .bind(user_uuid)
        .bind(search)
        .fetch(database, page, per_page)
        .await
    }

    /// Creates a server from this template: same egg, variables, limits and
    /// schedules as the snapshot, on `node_uuid` with fresh allocations.
    /// Like cloning, this never reuses an allocation or an external id.
    #[allow(clippy::too_many_arguments)]
    pub async fn instantiate(
        &self,
        state: &crate::State,
        name: compact_str::CompactString,
        description: Option<compact_str::CompactString>,
        owner_uuid: uuid::Uuid,
        node_uuid: uuid::Uuid,
        allocation_uuid: Option<uuid::Uuid>,
        allocation_uuids: Vec<uuid::Uuid>,
        start_on_completion: bool,
    ) -> Result<super::server::Server, crate::database::DatabaseError> {
        let snapshot = &self.snapshot;

        let egg_variables =
            super::nest_egg_variable::NestEggVariable::all_by_egg_uuid(&state.database, snapshot.egg_uuid)
                .await?;
        let variables = snapshot
            .variables
            .iter()
            .filter_map(|(env_variable, value)| {
                egg_variables
                    .iter()
                    .find(|variable| variable.env_variable == *env_variable)
                    .map(|variable| (variable.uuid, value.clone()))
            })
            .collect::<std::collections::HashMap<_, _>>();

        let (allocation_uuid, allocation_uuids) =
            if allocation_uuid.is_some() || !allocation_uuids.is_empty() {
                (allocation_uuid, allocation_uuids)
            } else {
                let mut deployment_variables = std::collections::HashMap::new();
                let (primary, additional) = super::server::Server::fresh_allocations(
                    state,
                    node_uuid,
                    snapshot.egg_uuid,
                    snapshot.allocation_count.max(1),
                    &[],
                    &mut deployment_variables,
                )
                .await?;

                let mut variables = variables;
                for (env_variable, value) in deployment_variables {
                    if let Some(variable) = egg_variables
                        .iter()
                        .find(|variable| variable.env_variable == env_variable)
                    {
                        variables.insert(variable.uuid, value);
                    }
                }

                // Rebuild `variables` with deployment overrides applied.
                let _ = &variables;
                return self
                    .instantiate_with_variables(
                        state,
                        name,
                        description,
                        owner_uuid,
                        node_uuid,
                        primary,
                        additional,
                        variables,
                        start_on_completion,
                    )
                    .await;
            };

        self.instantiate_with_variables(
            state,
            name,
            description,
            owner_uuid,
            node_uuid,
            allocation_uuid,
            allocation_uuids,
            variables,
            start_on_completion,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn instantiate_with_variables(
        &self,
        state: &crate::State,
        name: compact_str::CompactString,
        description: Option<compact_str::CompactString>,
        owner_uuid: uuid::Uuid,
        node_uuid: uuid::Uuid,
        allocation_uuid: Option<uuid::Uuid>,
        allocation_uuids: Vec<uuid::Uuid>,
        variables: std::collections::HashMap<uuid::Uuid, compact_str::CompactString>,
        start_on_completion: bool,
    ) -> Result<super::server::Server, crate::database::DatabaseError> {
        let snapshot = &self.snapshot;

        let server = super::server::Server::create(
            state,
            super::server::CreateServerOptions {
                node_uuid,
                owner_uuid,
                egg_uuid: snapshot.egg_uuid,
                backup_configuration_uuid: snapshot.backup_configuration_uuid,
                allocation_uuid,
                allocation_uuids,
                start_on_completion,
                skip_installer: false,
                external_id: None,
                name,
                description,
                limits: snapshot.limits,
                pinned_cpus: snapshot.pinned_cpus.clone(),
                startup: snapshot.startup.clone(),
                image: snapshot.image.clone(),
                labels: snapshot.labels.clone(),
                timezone: snapshot
                    .timezone
                    .as_deref()
                    .and_then(|timezone| timezone.parse::<chrono_tz::Tz>().ok()),
                trash: snapshot.trash,
                hugepages_passthrough_enabled: snapshot.hugepages_passthrough_enabled,
                kvm_passthrough_enabled: snapshot.kvm_passthrough_enabled,
                feature_limits: snapshot.feature_limits.clone(),
                variables,
            },
        )
        .await?;

        for exported in &snapshot.schedules {
            if server.schedule_limit <= 0 {
                break;
            }

            let schedule = match super::server_schedule::ServerSchedule::create(
                state,
                super::server_schedule::CreateServerScheduleOptions {
                    server_uuid: server.uuid,
                    name: exported.name.clone(),
                    enabled: exported.enabled,
                    triggers: exported.triggers.clone(),
                    condition: exported.condition.clone(),
                },
            )
            .await
            {
                Ok(schedule) => schedule,
                Err(err) => {
                    tracing::warn!(
                        template = %self.uuid,
                        server = %server.uuid,
                        "failed to create schedule from template: {err:#?}"
                    );
                    continue;
                }
            };

            for step in &exported.steps {
                if let Err(err) = super::server_schedule_step::ServerScheduleStep::create(
                    state,
                    super::server_schedule_step::CreateServerScheduleStepOptions {
                        schedule_uuid: schedule.uuid,
                        action: step.action.clone(),
                        order: step.order,
                    },
                )
                .await
                {
                    tracing::warn!(
                        template = %self.uuid,
                        server = %server.uuid,
                        schedule = %schedule.uuid,
                        "failed to create schedule step from template: {err:#?}"
                    );
                }
            }
        }

        Ok(server)
    }

    pub async fn by_uuid_visible(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
        user_uuid: uuid::Uuid,
        is_admin: bool,
    ) -> Result<Option<Self>, crate::database::DatabaseError> {
        let template = Self::by_uuid_optional(database, uuid).await?;

        Ok(template.filter(|template| {
            is_admin || template.owner_uuid.is_none_or(|owner| owner == user_uuid)
        }))
    }
}

#[async_trait::async_trait]
impl ByUuid for ServerTemplate {
    async fn by_uuid(
        database: &crate::database::Database,
        uuid: uuid::Uuid,
    ) -> Result<Self, crate::database::DatabaseError> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"
            SELECT {}
            FROM server_templates
            LEFT JOIN users ON users.uuid = server_templates.owner_uuid
            LEFT JOIN roles ON roles.uuid = users.role_uuid
            JOIN nest_eggs ON nest_eggs.uuid = (server_templates.snapshot->>'egg_uuid')::uuid
            WHERE server_templates.uuid = $1
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
            FROM server_templates
            LEFT JOIN users ON users.uuid = server_templates.owner_uuid
            LEFT JOIN roles ON roles.uuid = users.role_uuid
            JOIN nest_eggs ON nest_eggs.uuid = (server_templates.snapshot->>'egg_uuid')::uuid
            WHERE server_templates.uuid = $1
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
pub struct CreateServerTemplateOptions {
    #[garde(skip)]
    pub server_uuid: uuid::Uuid,
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub name: compact_str::CompactString,
    #[garde(length(chars, min = 1, max = 1024))]
    #[schema(min_length = 1, max_length = 1024)]
    pub description: Option<compact_str::CompactString>,
    /// `None` creates a global template (admin only); `Some(user)` a personal one.
    #[garde(skip)]
    pub owner_uuid: Option<uuid::Uuid>,
}

#[async_trait::async_trait]
impl CreatableModel for ServerTemplate {
    type CreateOptions<'a> = CreateServerTemplateOptions;
    type CreateResult = Self;

    fn get_create_handlers() -> &'static LazyLock<CreateListenerList<Self>> {
        static CREATE_LISTENERS: LazyLock<CreateListenerList<ServerTemplate>> =
            LazyLock::new(|| Arc::new(ModelHandlerList::default()));

        &CREATE_LISTENERS
    }

    async fn create_with_transaction(
        state: &crate::State,
        mut options: Self::CreateOptions<'_>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, crate::database::DatabaseError> {
        options.validate()?;

        let server = super::server::Server::by_uuid(&state.database, options.server_uuid).await?;
        let snapshot = ServerTemplateSnapshot::from_server(&state.database, &server).await?;
        snapshot.validate()?;

        if let Some(owner_uuid) = options.owner_uuid {
            super::user::User::by_uuid_optional(&state.database, owner_uuid)
                .await?
                .ok_or(crate::database::InvalidRelationError("owner"))?;
        }

        let mut query_builder = InsertQueryBuilder::new("server_templates");

        Self::run_create_handlers(&mut options, &mut query_builder, state, transaction).await?;

        query_builder
            .set("name", &options.name)
            .set("description", &options.description)
            .set("owner_uuid", options.owner_uuid)
            .set("source_server_uuid", options.server_uuid)
            .set("snapshot", serde_json::to_value(&snapshot)?);

        // The select list joins users/nest_eggs, so it cannot be used as a
        // RETURNING list; fetch the row back like Server::create does.
        let row = query_builder
            .returning("uuid")
            .fetch_one(&mut **transaction)
            .await?;
        let uuid: uuid::Uuid = row.try_get("uuid")?;
        let mut template = Self::by_uuid_with_transaction(transaction, uuid).await?;

        Self::run_after_create_handlers(&mut template, &options, state, transaction).await?;

        Ok(template)
    }
}

#[derive(ToSchema, Serialize, Deserialize, Validate, Clone, Default)]
pub struct UpdateServerTemplateOptions {
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub name: Option<compact_str::CompactString>,
    #[garde(length(chars, min = 1, max = 1024))]
    #[schema(min_length = 1, max_length = 1024)]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::double_option"
    )]
    pub description: Option<Option<compact_str::CompactString>>,
}

#[async_trait::async_trait]
impl UpdatableModel for ServerTemplate {
    type UpdateOptions = UpdateServerTemplateOptions;

    fn get_update_handlers() -> &'static LazyLock<UpdateHandlerList<Self>> {
        static UPDATE_LISTENERS: LazyLock<UpdateHandlerList<ServerTemplate>> =
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

        let mut query_builder = UpdateQueryBuilder::new("server_templates");

        self.run_update_handlers(&mut options, &mut query_builder, state, transaction)
            .await?;

        query_builder
            .set("name", options.name.as_ref())
            .set(
                "description",
                options.description.as_ref().map(|d| d.as_ref()),
            )
            .where_eq("uuid", self.uuid);

        query_builder.execute(&mut **transaction).await?;

        if let Some(name) = options.name {
            self.name = name;
        }
        if let Some(description) = options.description {
            self.description = description;
        }

        self.run_after_update_handlers(state, transaction).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl DeletableModel for ServerTemplate {
    type DeleteOptions = ();

    fn get_delete_handlers() -> &'static LazyLock<DeleteHandlerList<Self>> {
        static DELETE_LISTENERS: LazyLock<DeleteHandlerList<ServerTemplate>> =
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
            DELETE FROM server_templates
            WHERE server_templates.uuid = $1
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
#[init_args(ServerTemplate, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "ServerTemplate")]
pub struct ApiServerTemplate {
    pub uuid: uuid::Uuid,

    pub name: compact_str::CompactString,
    pub description: Option<compact_str::CompactString>,

    /// `None` means this is a global template, usable by everyone.
    pub owner_uuid: Option<uuid::Uuid>,
    pub is_global: bool,

    pub source_server_uuid: Option<uuid::Uuid>,
    pub egg_uuid: uuid::Uuid,
    pub egg_name: compact_str::CompactString,
    pub allocation_count: i32,
    pub schedule_count: i32,

    pub created: chrono::DateTime<chrono::Utc>,
    pub updated: chrono::DateTime<chrono::Utc>,
}

#[schema_extension_derive::extendible]
#[init_args(ServerTemplate, crate::State)]
#[hook_args(crate::State)]
#[derive(ToSchema, Serialize)]
#[schema(title = "AdminServerTemplate")]
pub struct AdminApiServerTemplate {
    #[serde(flatten)]
    pub template: ApiServerTemplate,

    pub owner: Option<super::user::AdminApiUser>,
    pub snapshot: ServerTemplateSnapshot,
}

#[async_trait::async_trait]
impl IntoApiObject for ServerTemplate {
    type ApiObject = ApiServerTemplate;
    type ExtraArgs<'a> = ();

    async fn into_api_object<'a>(
        self,
        state: &crate::State,
        _args: Self::ExtraArgs<'a>,
    ) -> Result<Self::ApiObject, crate::database::DatabaseError> {
        let api_object = ApiServerTemplate::init_hooks(&self, state).await?;

        let api_object = finish_extendible!(
            ApiServerTemplate {
                uuid: self.uuid,
                name: self.name,
                description: self.description,
                owner_uuid: self.owner_uuid,
                is_global: self.owner_uuid.is_none(),
                source_server_uuid: self.source_server_uuid,
                egg_uuid: self.snapshot.egg_uuid,
                egg_name: self.egg.name.clone(),
                allocation_count: self.snapshot.allocation_count,
                schedule_count: self.snapshot.schedules.len() as i32,
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
impl IntoAdminApiObject for ServerTemplate {
    type AdminApiObject = AdminApiServerTemplate;
    type ExtraArgs<'a> = &'a crate::storage::StorageUrlRetriever<'a>;

    async fn into_admin_api_object<'a>(
        self,
        state: &crate::State,
        storage_url_retriever: Self::ExtraArgs<'a>,
    ) -> Result<Self::AdminApiObject, crate::database::DatabaseError> {
        let api_object = AdminApiServerTemplate::init_hooks(&self, state).await?;
        let api_template = self.clone().into_api_object(state, ()).await?;

        let owner = match self.owner {
            Some(owner) => Some(
                owner
                    .into_admin_api_object(state, storage_url_retriever)
                    .await?,
            ),
            None => None,
        };

        let api_object = finish_extendible!(
            AdminApiServerTemplate {
                template: api_template,
                owner,
                snapshot: self.snapshot,
            },
            api_object,
            state
        )?;

        Ok(api_object)
    }
}
