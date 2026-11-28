//! Smart node picking and node resource alerts.
//!
//! Scoring is based on the panel's own allocated utilisation — the same
//! `server_usage` aggregate (`SUM(memory + memory_overhead)`, `SUM(disk)`,
//! `SUM(cpu)` over a node's servers) that powers
//! [`Node::by_location_uuids_most_eligible`](crate::models::node::Node::by_location_uuids_most_eligible)
//! and the `/api/admin/nodes/capacities` endpoint — plus a count of free
//! (unassigned) allocations per node. No live Wings call is made here, so
//! scoring is deterministic and cheap; live per-server usage
//! ([`Node::fetch_server_resources`](crate::models::node::Node::fetch_server_resources))
//! is a 15-second-cached Wings call and is deliberately not used for
//! placement decisions.
//!
//! Notes on the inputs:
//! * Nodes have a total memory and disk capacity, but no total CPU capacity
//!   (CPU is only a per-server limit, `100` = one core), so CPU can only be
//!   scored *relative* to the other candidates: the least CPU-loaded node in
//!   the candidate set scores best on CPU.
//! * `memory == 0` / `disk == 0` means unlimited, matching the existing
//!   deployment-eligibility query.
//!
//! The pure scoring core works on [`NodeScoreInput`] (plain numbers, no
//! database types) so it is unit-testable without a `Node` row;
//! [`NodeUsage`] is the database-backed view that converts into it.

use crate::{database::DatabaseError, models::node::Node, prelude::*};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use utoipa::ToSchema;

/// Warn when a node's allocated memory or disk reaches this share of its
/// total. Recovery (alert cleared) happens below [`CLEAR_THRESHOLD`], giving
/// hysteresis so a node hovering at the boundary does not flap.
pub const WARN_THRESHOLD: f64 = 0.85;
/// A node is considered recovered once its usage drops below this share.
pub const CLEAR_THRESHOLD: f64 = 0.80;

/// How long a raised resource alert stays suppressed before it may fire
/// again for the same node + resource, in seconds (6 hours).
pub const ALERT_COOLDOWN_SECONDS: u64 = 6 * 60 * 60;

/// Cache key tracking a raised alert for one node + resource. The cached
/// value is the uuid of the [`Announcement`](crate::models::announcement::Announcement)
/// created for the alert, so the recovery pass can dismiss it again.
pub fn alert_cache_key(node_uuid: uuid::Uuid, resource: ResourceKind) -> String {
    format!("node-resource-alerts::{node_uuid}::{}", resource.as_str())
}

/// Which allocated resource an alert is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Memory,
    Disk,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Disk => "disk",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "RAM",
            Self::Disk => "disk",
        }
    }
}

/// Plain numbers the scorer needs. Built from a [`NodeUsage`] in production
/// and by hand in tests.
#[derive(Debug, Clone, Copy)]
pub struct NodeScoreInput {
    pub memory: i64,
    pub disk: i64,
    pub used_memory: i64,
    pub used_disk: i64,
    pub used_cpu: i64,
    pub free_allocations: i64,
    pub deployment_enabled: bool,
    pub maintenance_enabled: bool,
}

impl NodeScoreInput {
    /// Share of total memory already allocated (0.0 when unlimited).
    pub fn memory_used_ratio(&self) -> f64 {
        if self.memory == 0 {
            0.0
        } else {
            self.used_memory as f64 / self.memory as f64
        }
    }

    /// Share of total disk already allocated (0.0 when unlimited).
    pub fn disk_used_ratio(&self) -> f64 {
        if self.disk == 0 {
            0.0
        } else {
            self.used_disk as f64 / self.disk as f64
        }
    }

    /// Free memory (MB) after the requested amount; negative when it does
    /// not fit. `i64::MAX` stands in for unlimited.
    pub fn memory_free_after(&self, requested_memory: i64) -> i64 {
        if self.memory == 0 {
            i64::MAX
        } else {
            self.memory - self.used_memory - requested_memory
        }
    }

    /// Free disk (MB) after the requested amount; negative when it does not
    /// fit. `i64::MAX` stands in for unlimited.
    pub fn disk_free_after(&self, requested_disk: i64) -> i64 {
        if self.disk == 0 {
            i64::MAX
        } else {
            self.disk - self.used_disk - requested_disk
        }
    }

    /// Resources currently at or above [`WARN_THRESHOLD`].
    pub fn over_threshold_resources(&self) -> Vec<ResourceKind> {
        let mut resources = Vec::new();
        if self.memory_used_ratio() >= WARN_THRESHOLD {
            resources.push(ResourceKind::Memory);
        }
        if self.disk_used_ratio() >= WARN_THRESHOLD {
            resources.push(ResourceKind::Disk);
        }
        resources
    }

    /// Resources currently below [`CLEAR_THRESHOLD`] (recovered).
    pub fn recovered_resources(&self) -> Vec<ResourceKind> {
        let mut resources = Vec::new();
        if self.memory_used_ratio() < CLEAR_THRESHOLD {
            resources.push(ResourceKind::Memory);
        }
        if self.disk_used_ratio() < CLEAR_THRESHOLD {
            resources.push(ResourceKind::Disk);
        }
        resources
    }
}

/// Allocated utilisation of one node, as fetched from the database.
#[derive(Debug, Clone)]
pub struct NodeUsage {
    pub node: Node,
    /// Allocated memory (MB), including memory overhead, suspended servers
    /// weighted by `suspension_penalty`.
    pub used_memory: i64,
    /// Allocated disk (MB).
    pub used_disk: i64,
    /// Allocated CPU (100 = one core).
    pub used_cpu: i64,
    pub servers: i64,
    /// Allocations on this node that are not assigned to any server.
    pub free_allocations: i64,
}

impl NodeUsage {
    pub fn score_input(&self) -> NodeScoreInput {
        NodeScoreInput {
            memory: self.node.memory,
            disk: self.node.disk,
            used_memory: self.used_memory,
            used_disk: self.used_disk,
            used_cpu: self.used_cpu,
            free_allocations: self.free_allocations,
            deployment_enabled: self.node.deployment_enabled,
            maintenance_enabled: self.node.maintenance_enabled,
        }
    }

    /// Share of total memory already allocated (0.0 when unlimited).
    pub fn memory_used_ratio(&self) -> f64 {
        self.score_input().memory_used_ratio()
    }

    /// Share of total disk already allocated (0.0 when unlimited).
    pub fn disk_used_ratio(&self) -> f64 {
        self.score_input().disk_used_ratio()
    }

    /// Free memory (MB) after the requested amount; `i64::MAX` = unlimited.
    pub fn memory_free_after(&self, requested_memory: i64) -> i64 {
        self.score_input().memory_free_after(requested_memory)
    }

    /// Free disk (MB) after the requested amount; `i64::MAX` = unlimited.
    pub fn disk_free_after(&self, requested_disk: i64) -> i64 {
        self.score_input().disk_free_after(requested_disk)
    }

    /// Resources currently at or above [`WARN_THRESHOLD`].
    pub fn over_threshold_resources(&self) -> Vec<ResourceKind> {
        self.score_input().over_threshold_resources()
    }

    /// Resources currently below [`CLEAR_THRESHOLD`] (recovered).
    pub fn recovered_resources(&self) -> Vec<ResourceKind> {
        self.score_input().recovered_resources()
    }
}

/// The score of one node for a deployment, plus why it scored that way.
#[derive(Debug, Clone)]
pub struct ScoredNode {
    pub usage: NodeUsage,
    /// 0–100. Higher is better. `None` when the node cannot take the server.
    pub score: Option<f64>,
    /// Why the node is ineligible, when [`Self::score`] is `None`.
    pub ineligible_reason: Option<compact_str::CompactString>,
    /// Non-fatal warnings that lowered the score (e.g. over 85% used).
    pub warnings: Vec<compact_str::CompactString>,
    /// Share of total memory that would still be free after this deployment.
    pub memory_free_ratio: f64,
    /// Share of total disk that would still be free after this deployment.
    pub disk_free_ratio: f64,
}

impl ScoredNode {
    #[inline]
    pub fn eligible(&self) -> bool {
        self.score.is_some()
    }

    /// Human-readable reason, e.g. `"Most free RAM: 12.4 GiB free (78% free)"`.
    pub fn reason(&self) -> String {
        let memory_free = self.usage.memory_free_after(0);
        let disk_free = self.usage.disk_free_after(0);
        let memory_free_label = format_mb(memory_free);
        let disk_free_label = format_mb(disk_free);
        let memory_free_percent = (self.memory_free_ratio * 100.0).round() as i64;
        let disk_free_percent = (self.disk_free_ratio * 100.0).round() as i64;

        if self.memory_free_ratio >= self.disk_free_ratio {
            format!(
                "Most free RAM: {memory_free_label} free ({memory_free_percent}% free) · {disk_free_label} disk free ({disk_free_percent}% free)"
            )
        } else {
            format!(
                "Most free disk: {disk_free_label} free ({disk_free_percent}% free) · {memory_free_label} RAM free ({memory_free_percent}% free)"
            )
        }
    }
}

/// Format megabytes as a human-readable GiB/MiB string. `i64::MAX` (used for
/// unlimited totals) renders as "unlimited".
pub fn format_mb(megabytes: i64) -> String {
    if megabytes == i64::MAX {
        return "unlimited".to_string();
    }
    if megabytes >= 1024 {
        format!("{:.1} GiB", megabytes as f64 / 1024.0)
    } else {
        format!("{megabytes} MiB")
    }
}

/// Pure scoring of one candidate. Returns
/// `(score, ineligible_reason, warnings, memory_free_ratio, disk_free_ratio)`.
/// `max_used_cpu` is the highest allocated CPU across the candidate set —
/// CPU has no node total, so it is scored relative to the most-loaded
/// candidate (see the module docs).
///
/// Weights: free memory 50, free disk 30, relative CPU headroom 20.
/// Hard exclusions (score `None`): deployment disabled, maintenance mode,
/// no free allocations, and — unless `allow_overallocation` — not enough
/// free memory/disk for the request. A node at or above 85% used on RAM or
/// disk keeps its score but is halved (and quartered above 95%), so it is
/// only picked when nothing better exists.
pub fn score_input(
    input: NodeScoreInput,
    max_used_cpu: i64,
    requested_memory: i64,
    requested_disk: i64,
    allow_overallocation: bool,
) -> (
    Option<f64>,
    Option<compact_str::CompactString>,
    Vec<compact_str::CompactString>,
    f64,
    f64,
) {
    let mut warnings = Vec::new();

    if !input.deployment_enabled {
        return (
            None,
            Some("deployment is disabled on this node".into()),
            warnings,
            0.0,
            0.0,
        );
    }
    if input.maintenance_enabled {
        return (
            None,
            Some("node is under maintenance".into()),
            warnings,
            0.0,
            0.0,
        );
    }
    if input.free_allocations == 0 {
        return (
            None,
            Some("node has no free allocations".into()),
            warnings,
            0.0,
            0.0,
        );
    }

    let memory_free = input.memory_free_after(requested_memory);
    let disk_free = input.disk_free_after(requested_disk);
    if !allow_overallocation && memory_free < 0 {
        return (
            None,
            Some("node does not have enough free memory".into()),
            warnings,
            0.0,
            0.0,
        );
    }
    if !allow_overallocation && disk_free < 0 {
        return (
            None,
            Some("node does not have enough free disk".into()),
            warnings,
            0.0,
            0.0,
        );
    }

    // Free ratios after the deployment lands (1.0 when unlimited).
    let memory_free_ratio = if input.memory == 0 {
        1.0
    } else {
        (memory_free as f64 / input.memory as f64).clamp(0.0, 1.0)
    };
    let disk_free_ratio = if input.disk == 0 {
        1.0
    } else {
        (disk_free as f64 / input.disk as f64).clamp(0.0, 1.0)
    };
    let cpu_score = if max_used_cpu <= 0 {
        1.0
    } else {
        1.0 - (input.used_cpu.max(0) as f64 / max_used_cpu as f64)
    };

    let mut score = 50.0 * memory_free_ratio + 30.0 * disk_free_ratio + 20.0 * cpu_score;

    // Penalise — but do not exclude — nodes already running hot, so the
    // picker prefers headroom and only uses a nearly-full node when nothing
    // better exists.
    let highest_used = input
        .memory_used_ratio()
        .max(input.disk_used_ratio());
    if highest_used >= 0.95 {
        score *= 0.25;
        warnings.push(compact_str::format_compact!(
            "node is over 95% used ({:.0}%)",
            highest_used * 100.0
        ));
    } else if highest_used >= WARN_THRESHOLD {
        score *= 0.5;
        warnings.push(compact_str::format_compact!(
            "node is over 85% used ({:.0}%)",
            highest_used * 100.0
        ));
    }

    (
        Some(score.clamp(0.0, 100.0)),
        None,
        warnings,
        memory_free_ratio,
        disk_free_ratio,
    )
}

/// Score every candidate node for a server needing `requested_memory` MB
/// of RAM and `requested_disk` MB of disk.
pub fn score_nodes(
    usages: Vec<NodeUsage>,
    requested_memory: i64,
    requested_disk: i64,
    allow_overallocation: bool,
) -> Vec<ScoredNode> {
    let max_used_cpu = usages
        .iter()
        .map(|usage| usage.used_cpu)
        .max()
        .unwrap_or(0)
        .max(0);

    usages
        .into_iter()
        .map(|usage| {
            let (score, ineligible_reason, warnings, memory_free_ratio, disk_free_ratio) =
                score_input(
                    usage.score_input(),
                    max_used_cpu,
                    requested_memory,
                    requested_disk,
                    allow_overallocation,
                );

            ScoredNode {
                usage,
                score,
                ineligible_reason,
                warnings,
                memory_free_ratio,
                disk_free_ratio,
            }
        })
        .collect()
}

/// Score candidates and return them best-first. Ineligible nodes sort last,
/// in their original order.
pub fn recommend_nodes(
    usages: Vec<NodeUsage>,
    requested_memory: i64,
    requested_disk: i64,
    allow_overallocation: bool,
) -> Vec<ScoredNode> {
    let mut scored = score_nodes(usages, requested_memory, requested_disk, allow_overallocation);
    scored.sort_by(|a, b| {
        b.score
            .unwrap_or(f64::NEG_INFINITY)
            .total_cmp(&a.score.unwrap_or(f64::NEG_INFINITY))
    });
    scored
}

/// The single best eligible node, if any.
pub fn recommend_node(
    usages: Vec<NodeUsage>,
    requested_memory: i64,
    requested_disk: i64,
    allow_overallocation: bool,
) -> Option<ScoredNode> {
    recommend_nodes(usages, requested_memory, requested_disk, allow_overallocation)
        .into_iter()
        .find(|scored| scored.eligible())
}

/// Fetch allocated utilisation for nodes in the given locations. Pass an
/// empty slice for all locations. Suspended servers count at
/// `suspension_penalty` (0.0–1.0) of their limits, matching
/// `Node::by_location_uuids_most_eligible`.
pub async fn fetch_node_usages(
    database: &crate::database::Database,
    location_uuids: &[uuid::Uuid],
    suspension_penalty: f64,
) -> Result<Vec<NodeUsage>, DatabaseError> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"
        WITH server_usage AS (
            SELECT
                node_uuid,
                COALESCE(SUM((memory + memory_overhead) * CASE WHEN suspended THEN $2 ELSE 1.0 END), 0)::BIGINT AS used_memory,
                COALESCE(SUM(disk * CASE WHEN suspended THEN $2 ELSE 1.0 END), 0)::BIGINT AS used_disk,
                COALESCE(SUM(cpu * CASE WHEN suspended THEN $2 ELSE 1.0 END), 0)::BIGINT AS used_cpu,
                COUNT(*)::BIGINT AS servers
            FROM servers
            GROUP BY node_uuid
        ),
        free_allocations AS (
            SELECT
                node_allocations.node_uuid,
                COUNT(*)::BIGINT AS free_allocations
            FROM node_allocations
            LEFT JOIN server_allocations ON server_allocations.allocation_uuid = node_allocations.uuid
            WHERE server_allocations.uuid IS NULL
            GROUP BY node_allocations.node_uuid
        )
        SELECT {}, COALESCE(u.used_memory, 0) AS used_memory, COALESCE(u.used_disk, 0) AS used_disk,
            COALESCE(u.used_cpu, 0) AS used_cpu, COALESCE(u.servers, 0) AS servers,
            COALESCE(f.free_allocations, 0) AS free_allocations
        FROM nodes
        JOIN locations ON locations.uuid = nodes.location_uuid
        LEFT JOIN server_usage u ON nodes.uuid = u.node_uuid
        LEFT JOIN free_allocations f ON nodes.uuid = f.node_uuid
        WHERE ($1::uuid[] IS NULL OR nodes.location_uuid = ANY($1))
        ORDER BY nodes.created, nodes.uuid
        "#,
        Node::columns_sql(None),
    )))
    .bind(if location_uuids.is_empty() {
        None
    } else {
        Some(location_uuids.to_vec())
    })
    .bind(suspension_penalty)
    .fetch_all(database.read())
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(NodeUsage {
                node: Node::map(None, &row)?,
                used_memory: row.try_get("used_memory")?,
                used_disk: row.try_get("used_disk")?,
                used_cpu: row.try_get("used_cpu")?,
                servers: row.try_get("servers")?,
                free_allocations: row.try_get("free_allocations")?,
            })
        })
        .collect()
}

/// Fetch allocated utilisation for every node (used by the resource-alert
/// background task).
pub async fn fetch_all_node_usages(
    database: &crate::database::Database,
) -> Result<Vec<NodeUsage>, DatabaseError> {
    fetch_node_usages(database, &[], 0.5).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        memory: i64,
        used_memory: i64,
        disk: i64,
        used_disk: i64,
        used_cpu: i64,
        free_allocations: i64,
    ) -> NodeScoreInput {
        NodeScoreInput {
            memory,
            disk,
            used_memory,
            used_disk,
            used_cpu,
            free_allocations,
            deployment_enabled: true,
            maintenance_enabled: false,
        }
    }

    #[test]
    fn emptiest_node_scores_highest() {
        let full = input(8192, 7000, 100_000, 80_000, 400, 3);
        let empty = input(8192, 1000, 100_000, 10_000, 100, 3);
        let max_cpu = 400;

        let (full_score, _, _, _, _) = score_input(full, max_cpu, 1024, 10_240, false);
        let (empty_score, _, _, _, _) = score_input(empty, max_cpu, 1024, 10_240, false);

        assert!(empty_score.unwrap() > full_score.unwrap());
        assert!(empty_score.unwrap() > 50.0);
    }

    #[test]
    fn maintenance_disabled_and_unallocated_nodes_are_ineligible() {
        let mut maintenance = input(8192, 1000, 100_000, 10_000, 100, 3);
        maintenance.maintenance_enabled = true;
        let (score, reason, _, _, _) = score_input(maintenance, 100, 1024, 10_240, false);
        assert!(score.is_none());
        assert!(reason.unwrap().contains("maintenance"));

        let mut disabled = input(8192, 1000, 100_000, 10_000, 100, 3);
        disabled.deployment_enabled = false;
        let (score, reason, _, _, _) = score_input(disabled, 100, 1024, 10_240, false);
        assert!(score.is_none());
        assert!(reason.unwrap().contains("disabled"));

        let no_allocations = input(8192, 1000, 100_000, 10_000, 100, 0);
        let (score, reason, _, _, _) = score_input(no_allocations, 100, 1024, 10_240, false);
        assert!(score.is_none());
        assert!(reason.unwrap().contains("allocations"));
    }

    #[test]
    fn over_85_percent_node_is_penalised_not_excluded() {
        let hot = input(8192, 7200, 100_000, 10_000, 100, 3);
        let (score, reason, warnings, _, _) = score_input(hot, 100, 512, 1024, false);

        assert!(score.is_some());
        assert!(reason.is_none());
        assert!(warnings.iter().any(|w| w.contains("85%")));
        // Halved: without the penalty the base score would be above 60.
        assert!(score.unwrap() < 45.0);
    }

    #[test]
    fn node_without_room_is_ineligible_unless_overallocation_allowed() {
        let small = input(2048, 1500, 100_000, 10_000, 100, 3);
        let (score, reason, _, _, _) = score_input(small, 100, 1024, 10_240, false);
        assert!(score.is_none());
        assert!(reason.unwrap().contains("memory"));

        let (score, _, _, _, _) = score_input(small, 100, 1024, 10_240, true);
        assert!(score.is_some());
    }

    #[test]
    fn thresholds_have_hysteresis() {
        let hot = input(10_000, 8600, 10_000, 1000, 100, 3);
        assert_eq!(hot.over_threshold_resources(), vec![ResourceKind::Memory]);
        assert_eq!(hot.recovered_resources(), vec![ResourceKind::Disk]);

        let cooling = input(10_000, 7900, 10_000, 1000, 100, 3);
        assert!(cooling.over_threshold_resources().is_empty());
        assert!(cooling.recovered_resources().contains(&ResourceKind::Memory));
    }

    #[test]
    fn format_mb_renders_gib_and_unlimited() {
        assert_eq!(format_mb(12_288), "12.0 GiB");
        assert_eq!(format_mb(512), "512 MiB");
        assert_eq!(format_mb(i64::MAX), "unlimited");
    }
}
