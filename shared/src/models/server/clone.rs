//! Server cloning.
//!
//! `Server` deliberately does **not** implement [`DuplicableModel`]: its
//! `create_with_transaction` is explicitly unsupported, because
//! [`Server::create`] owns its own transaction *and* the Wings `POST /servers`
//! call that provisions the server on the node. A `duplicate_with_transaction`
//! implementation could not reuse that path. Cloning therefore lives here as
//! `Server::clone_server`, which reuses `Server::create` directly and then
//! duplicates the source server's schedules through the existing
//! [`DuplicableModel`] implementations on [`ServerSchedule`] (whose duplicate
//! also copies its steps).
//!
//! Allocation rule, enforced throughout: the source server's allocations are
//! **never** reused. The clone always gets fresh allocations on the target
//! node, picked the same way the deploy endpoint picks them (egg deployment
//! config first, random free allocations as fallback).

use super::{
    AdminApiServerLimits, ApiServerBandwidth, ApiServerFeatureLimits, CreateServerOptions,
    Server, ServerStatus,
};
use crate::{
    models::{
        DuplicableModel,
        egg_configuration::{EggConfiguration, EggConfigurationTarget},
        nest_egg_variable::NestEggVariable,
        node::Node,
        node_allocation::NodeAllocation,
        server_backup::{CreateServerBackupOptions, ServerBackup, ServerBackupRestoreOptions},
        server_schedule::{DuplicateServerScheduleOptions, ServerSchedule},
        server_variable::ServerVariable,
    },
    prelude::*,
    response::DisplayError,
};
use garde::Validate;
use serde::Deserialize;
use std::collections::HashMap;
use utoipa::ToSchema;

#[derive(ToSchema, Validate, Deserialize, Clone)]
pub struct CloneServerOptions {
    #[garde(length(chars, min = 1, max = 255))]
    #[schema(min_length = 1, max_length = 255)]
    pub name: compact_str::CompactString,
    #[garde(length(chars, min = 1, max = 1024))]
    #[schema(min_length = 1, max_length = 1024)]
    pub description: Option<compact_str::CompactString>,

    /// Defaults to the source server's owner. Admins may pass a different
    /// owner; the client endpoint ignores/forbids this and keeps the owner.
    #[garde(skip)]
    pub owner_uuid: Option<uuid::Uuid>,
    /// Defaults to the source server's node.
    #[garde(skip)]
    pub node_uuid: Option<uuid::Uuid>,

    /// Explicit fresh allocations to use instead of auto-picking. They must
    /// belong to the target node and be unassigned; `Server::create` rejects
    /// an already-assigned one with a unique violation.
    #[garde(skip)]
    pub allocation_uuid: Option<uuid::Uuid>,
    #[garde(skip)]
    pub allocation_uuids: Vec<uuid::Uuid>,

    /// Copy the source server's files. Default `false`. Implemented by taking
    /// a temporary backup of the source and restoring it onto the clone (see
    /// [`Server::clone_server`]); the clone is created with the installer
    /// skipped so the restore is not overwritten, and is left in
    /// `restoring_backup` status until the copy finishes in the background.
    /// Requires a backup configuration for the source server.
    #[garde(skip)]
    #[serde(default)]
    pub include_files: bool,
    /// Copy the source server's schedules (and their steps). Default `true`.
    #[garde(skip)]
    #[serde(default = "default_include_schedules")]
    pub include_schedules: bool,
    #[garde(skip)]
    #[serde(default)]
    pub start_on_completion: bool,
}

fn default_include_schedules() -> bool {
    true
}

impl Server {
    /// Picks fresh allocations on `node_uuid` for a server that needs `count`
    /// allocations (primary + additional). Never returns an allocation the
    /// source server (or any other server) already holds.
    ///
    /// Prefers the egg's deployment config (same code path as
    /// `POST /api/admin/servers/deploy` via
    /// [`NodeAllocation::get_from_deployment`], which also fills in any
    /// deployment-assigned variables such as the game port), and falls back to
    /// random free allocations in the full port range.
    pub async fn fresh_allocations(
        state: &crate::State,
        node_uuid: uuid::Uuid,
        egg_uuid: uuid::Uuid,
        count: i32,
        exclude: &[uuid::Uuid],
        deployment_variables: &mut HashMap<compact_str::CompactString, compact_str::CompactString>,
    ) -> Result<(Option<uuid::Uuid>, Vec<uuid::Uuid>), crate::database::DatabaseError> {
        let node = Node::by_uuid(&state.database, node_uuid).await?;
        let egg = super::super::nest_egg::NestEgg::by_uuid(&state.database, egg_uuid).await?;

        let mut primary = None;
        let mut additional: Vec<uuid::Uuid> = Vec::new();

        let deployment = EggConfiguration::merged_for_target(
            &state.database,
            &EggConfigurationTarget::new(&egg, &node),
        )
        .await
        .ok()
        .and_then(|config| config.config_allocations)
        .map(|allocations| allocations.deployment);

        if let Some(deployment) = &deployment {
            let mut variables = HashMap::new();
            if let Ok((deployment_primary, deployment_additional)) =
                NodeAllocation::get_from_deployment(state, deployment, &node, &mut variables, None)
                    .await
            {
                primary = deployment_primary;
                additional = deployment_additional;

                for (env_variable, value) in variables {
                    deployment_variables.insert(env_variable.into(), value);
                }
            }
        }

        let have = primary.is_some() as i32 + additional.len() as i32;
        if have < count {
            let mut excluded: Vec<uuid::Uuid> = exclude.to_vec();
            excluded.extend(additional.iter().copied());
            if let Some(primary) = primary {
                excluded.push(primary);
            }

            let needed = count - have;
            let random = NodeAllocation::get_random(
                &state.database,
                node_uuid,
                1,
                u16::MAX,
                needed as i64,
                &excluded,
                None,
            )
            .await?;

            for allocation in random {
                if primary.is_none() {
                    primary = Some(allocation);
                } else {
                    additional.push(allocation);
                }
            }
        }

        Ok((primary, additional))
    }

    /// Creates a new server copying the source server's egg, startup command,
    /// image, labels, limits, feature limits, timezone, trash settings,
    /// passthrough flags, backup configuration, startup variables and (by
    /// default) schedules. Subusers, databases, backups, mounts and devices
    /// are deliberately **not** copied: they are credentials / node-local
    /// attachments, not server configuration.
    ///
    /// The new server always gets fresh allocations (see
    /// [`Server::fresh_allocations`]) and a fresh `external_id` of `None`;
    /// copying the source's external id would violate its uniqueness.
    pub async fn clone_server(
        state: &crate::State,
        source: &Server,
        options: CloneServerOptions,
    ) -> Result<Self, crate::database::DatabaseError> {
        options.validate()?;

        if source.destination_node.is_some() {
            return Err(crate::database::DatabaseError::Any(
                DisplayError::new("cannot clone a server that is being transferred")
                    .with_status(axum::http::StatusCode::CONFLICT)
                    .into(),
            ));
        }

        let node_uuid = options.node_uuid.unwrap_or(source.node.uuid);
        let owner_uuid = options.owner_uuid.unwrap_or(source.owner.uuid);

        if options.include_files
            && matches!(
                source.status,
                Some(ServerStatus::Installing) | Some(ServerStatus::RestoringBackup)
            )
        {
            return Err(crate::database::DatabaseError::Any(
                DisplayError::new(
                    "cannot copy files from a server that is installing or restoring a backup",
                )
                .with_status(axum::http::StatusCode::CONFLICT)
                .into(),
            ));
        }

        // Fail fast: file copying needs a backup configuration, and there is
        // no point creating the clone first if the copy can never run.
        if options.include_files
            && source
                .backup_configuration_for(&state.database, None)
                .await
                .is_none()
        {
            return Err(crate::database::DatabaseError::Any(
                DisplayError::new(
                    "cannot copy files: no backup configuration is available for this server",
                )
                .with_status(axum::http::StatusCode::EXPECTATION_FAILED)
                .into(),
            ));
        }

        let source_variables =
            ServerVariable::all_by_server_uuid_egg_uuid(&state.database, source.uuid, source.egg.uuid)
                .await?;
        let mut variables: HashMap<uuid::Uuid, compact_str::CompactString> = source_variables
            .iter()
            .map(|variable| (variable.variable.uuid, variable.value.clone().into()))
            .collect();

        let source_allocations: Vec<uuid::Uuid> = sqlx::query_scalar(
            r#"
            SELECT server_allocations.allocation_uuid
            FROM server_allocations
            WHERE server_allocations.server_uuid = $1
            "#,
        )
        .bind(source.uuid)
        .fetch_all(state.database.read())
        .await?;
        let allocation_count = source_allocations.len().max(1) as i32;

        let (allocation_uuid, allocation_uuids) =
            if options.allocation_uuid.is_some() || !options.allocation_uuids.is_empty() {
                (options.allocation_uuid, options.allocation_uuids)
            } else {
                let mut deployment_variables = HashMap::new();
                let (primary, additional) = Self::fresh_allocations(
                    state,
                    node_uuid,
                    source.egg.uuid,
                    allocation_count,
                    &source_allocations,
                    &mut deployment_variables,
                )
                .await?;

                // Deployment-assigned variables (e.g. the game port) refer to
                // the *new* allocations, so they override the copied values.
                if !deployment_variables.is_empty() {
                    let egg_variables =
                        NestEggVariable::all_by_egg_uuid(&state.database, source.egg.uuid).await?;

                    for (env_variable, value) in deployment_variables {
                        if let Some(variable) = egg_variables
                            .iter()
                            .find(|variable| variable.env_variable == env_variable)
                        {
                            variables.insert(variable.uuid, value);
                        }
                    }
                }

                (primary, additional)
            };

        let timezone = source
            .timezone
            .as_deref()
            .and_then(|timezone| timezone.parse::<chrono_tz::Tz>().ok());

        let clone = Self::create(
            state,
            CreateServerOptions {
                node_uuid,
                owner_uuid,
                egg_uuid: source.egg.uuid,
                backup_configuration_uuid: source.backup_configuration.map(|fetchable| fetchable.uuid),
                allocation_uuid,
                allocation_uuids,
                start_on_completion: options.start_on_completion,
                // With files being copied in, the installer must not run and
                // overwrite the restored files.
                skip_installer: options.include_files,
                external_id: None,
                name: options.name,
                description: options.description.or_else(|| source.description.clone()),
                limits: AdminApiServerLimits {
                    cpu: source.cpu,
                    memory: source.memory,
                    memory_overhead: source.memory_overhead,
                    swap: source.swap,
                    disk: source.disk,
                    io_weight: source.io_weight,
                    bandwidth: Some(ApiServerBandwidth {
                        upload: source.bandwidth_upload as u64,
                        download: source.bandwidth_download as u64,
                    }),
                },
                pinned_cpus: source.pinned_cpus.clone(),
                startup: source.startup.clone(),
                image: source.image.clone(),
                labels: source.labels.clone(),
                timezone,
                trash: source.trash,
                hugepages_passthrough_enabled: source.hugepages_passthrough_enabled,
                kvm_passthrough_enabled: source.kvm_passthrough_enabled,
                feature_limits: ApiServerFeatureLimits {
                    allocations: source.allocation_limit,
                    databases: source.database_limit,
                    backups: source.backup_limit,
                    schedules: source.schedule_limit,
                    // The extendible `__overlay` carries extension data; a
                    // clone only copies the four numeric limits, so it is
                    // empty here (no extension hooks run for this value).
                    // `ExtensionOverlay::new()` (not `Default::default()`):
                    // `Default` for the overlay is conditional on the
                    // extendible type itself implementing `Default`, which
                    // `ApiServerFeatureLimits` does not.
                    __overlay: schema_extension_core::ExtensionOverlay::new(),
                },
                variables,
            },
        )
        .await?;

        if options.include_schedules && source.schedule_limit > 0 {
            let mut page = 1;
            let mut copied = 0;
            loop {
                let page_result = ServerSchedule::by_server_uuid_with_pagination(
                    &state.database,
                    source.uuid,
                    page,
                    100,
                    None,
                )
                .await?;

                let done = (page_result.data.len() as i64) < page_result.per_page;
                for schedule in page_result.data {
                    if copied >= source.schedule_limit {
                        break;
                    }

                    if let Err(err) = ServerSchedule::duplicate(
                        &schedule,
                        state,
                        DuplicateServerScheduleOptions {
                            server_uuid: clone.uuid,
                            name: schedule.name.clone(),
                        },
                    )
                    .await
                    {
                        tracing::warn!(
                            source = %source.uuid,
                            clone = %clone.uuid,
                            schedule = %schedule.uuid,
                            "failed to copy schedule while cloning server: {err:#?}"
                        );
                        continue;
                    }

                    copied += 1;
                }

                if done || copied >= source.schedule_limit {
                    break;
                }
                page += 1;
            }
        }

        if options.include_files {
            Self::copy_files_in_background(state.clone(), source.clone(), clone.clone());
        }

        Ok(clone)
    }

    /// Takes a temporary backup of `source` and restores it onto `clone`.
    /// Runs in the background: backup creation on Wings is asynchronous, so
    /// the clone is returned to the caller first (in `restoring_backup`
    /// status) and the copy completes later. Failures are logged, never
    /// silent, and leave the clone usable with its fresh install skipped —
    /// the caller can retry by restoring a backup manually. The temporary
    /// backup is deleted once the restore finishes.
    fn copy_files_in_background(state: crate::State, source: Server, clone: Server) {
        tokio::spawn(async move {
            if let Err(err) =
                Self::try_set_status_by_uuid(
                    state.database.write(),
                    clone.uuid,
                    None,
                    Some(ServerStatus::RestoringBackup),
                )
                .await
            {
                tracing::error!(clone = %clone.uuid, "failed to mark cloned server as restoring: {err:#?}");
                return;
            }
            Self::invalidate_cached(&state.database, clone.uuid).await;

            let result = Self::copy_files(&state, &source, &clone).await;

            // Always clear the restoring status, even on failure.
            if let Err(err) = Self::try_set_status_by_uuid(
                state.database.write(),
                clone.uuid,
                Some(ServerStatus::RestoringBackup),
                None,
            )
            .await
            {
                tracing::error!(clone = %clone.uuid, "failed to clear restoring status on cloned server: {err:#?}");
            }
            Self::invalidate_cached(&state.database, clone.uuid).await;

            if let Err(err) = result {
                tracing::error!(source = %source.uuid, clone = %clone.uuid, "failed to copy files while cloning server: {err:#?}");
            }
        });
    }

    async fn copy_files(
        state: &crate::State,
        source: &Server,
        clone: &Server,
    ) -> Result<(), anyhow::Error> {
        let backup_configuration = source
            .backup_configuration_for(&state.database, None)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("no backup configuration is available for this server")
            })?;

        let backup = ServerBackup::create(
            state,
            CreateServerBackupOptions {
                server: source,
                name: format!("clone-files-{}", clone.uuid).into(),
                backup_group_uuid: None,
                system_backup_policy_uuid: None,
                database_instance: None,
                backup_configuration: Some(backup_configuration),
                ignored_files: Vec::new(),
                metadata: ServerBackup::generate_metadata(state, source).await?,
            },
        )
        .await?;

        // Wait for Wings to finish the backup (poll the row; creation is async).
        let backup = {
            let mut completed = None;
            for _ in 0..360 {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                let current = ServerBackup::by_uuid(&state.database, backup.uuid).await?;
                if current.completed.is_some() {
                    completed = Some(current);
                    break;
                }
            }

            let backup = completed
                .ok_or_else(|| anyhow::anyhow!("timed out waiting for the clone backup to complete"))?;
            if !backup.successful {
                return Err(anyhow::anyhow!(
                    "the temporary backup taken for cloning failed"
                ));
            }

            backup
        };

        let mut transaction = state.database.write().begin().await?;
        // `restore` takes `self` by value; clone so `backup` is still
        // available below for the cleanup delete + warning.
        if let Err(err) = backup.clone()
            .restore(
                state,
                &mut transaction,
                clone.clone(),
                ServerBackupRestoreOptions {
                    truncate_directory: true,
                    restore_startup: false,
                },
            )
            .await
        {
            transaction.rollback().await?;
            return Err(err);
        }
        transaction.commit().await?;

        if let Err(err) = backup.delete(state, Default::default()).await {
            tracing::warn!(backup = %backup.uuid, "failed to delete temporary clone backup: {err:#?}");
        }

        Ok(())
    }
}
