use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use axum::extract::Query;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        GetState,
        models::{
            IntoAdminApiObject,
            node::{AdminApiNode, Node},
            node_recommendation,
            user::GetPermissionManager,
        },
        node_recommendation::{ResourceKind, ScoredNode},
        response::{ApiResponse, ApiResponseResult},
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Params {
        /// Required memory (MB) for the server to place. Defaults to 0.
        #[serde(default)]
        #[garde(range(min = 0))]
        #[schema(minimum = 0, default = 0)]
        pub memory: i64,
        /// Required CPU (100 = one core). Only used for the relative CPU
        /// component of the score; nodes have no total CPU capacity.
        #[serde(default)]
        #[garde(range(min = 0))]
        #[schema(minimum = 0, default = 0)]
        pub cpu: i64,
        /// Required disk (MB) for the server to place. Defaults to 0.
        #[serde(default)]
        #[garde(range(min = 0))]
        #[schema(minimum = 0, default = 0)]
        pub disk: i64,
        /// Only consider nodes in these locations. Empty means all locations.
        #[serde(default)]
        #[garde(skip)]
        pub location_uuid: Vec<uuid::Uuid>,
        /// Allow recommending a node the request does not fit on.
        #[serde(default)]
        #[garde(skip)]
        pub allow_overallocation: bool,
        /// Suspended servers count at this share of their limits (0.0–1.0).
        #[serde(default = "default_suspension_penalty")]
        #[garde(range(min = 0.0, max = 1.0))]
        #[schema(minimum = 0.0, maximum = 1.0, default = 0.5)]
        pub suspension_penalty: f64,
    }

    fn default_suspension_penalty() -> f64 {
        0.5
    }

    #[derive(ToSchema, Serialize)]
    pub struct RecommendedNode {
        pub node: AdminApiNode,
        /// 0–100. Higher is better.
        pub score: f64,
        /// Human-readable reason, e.g. "Most free RAM: 12.4 GiB free (78% free)".
        pub reason: String,
        /// Non-fatal warnings that lowered the score.
        pub warnings: Vec<String>,
        pub memory_free_mb: i64,
        pub disk_free_mb: i64,
        /// Allocated memory as a share of the node total (0.0–1.0+).
        pub memory_used_ratio: f64,
        /// Allocated disk as a share of the node total (0.0–1.0+).
        pub disk_used_ratio: f64,
        /// Resources at or above the 85% warning threshold.
        pub over_threshold: Vec<ResourceKind>,
    }

    impl RecommendedNode {
        async fn from_scored(
            state: &GetState,
            scored: &ScoredNode,
        ) -> Result<Self, shared::database::DatabaseError> {
            Ok(Self {
                node: scored
                    .usage
                    .node
                    .clone()
                    .into_admin_api_object(state, ())
                    .await?,
                score: scored.score.unwrap_or(0.0),
                reason: scored.reason(),
                warnings: scored
                    .warnings
                    .iter()
                    .map(|warning| warning.to_string())
                    .collect(),
                memory_free_mb: scored.usage.memory_free_after(0),
                disk_free_mb: scored.usage.disk_free_after(0),
                memory_used_ratio: scored.usage.memory_used_ratio(),
                disk_used_ratio: scored.usage.disk_used_ratio(),
                over_threshold: scored.usage.over_threshold_resources(),
            })
        }
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        /// The best node for the request, or `null` when no node can take it.
        pub node: Option<RecommendedNode>,
        /// Why no node could be recommended, when `node` is `null`.
        pub reason: Option<String>,
        /// Next-best eligible nodes, best first (at most 4).
        pub alternatives: Vec<RecommendedNode>,
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
    ), params(
        ("memory" = i64, Query, description = "Required memory in MB"),
        ("cpu" = i64, Query, description = "Required CPU, 100 = one core"),
        ("disk" = i64, Query, description = "Required disk in MB"),
        ("location_uuid" = Vec<String>, Query, description = "Only consider nodes in these locations (repeatable)"),
        ("allow_overallocation" = bool, Query, description = "Allow recommending a node the request does not fit on"),
        ("suspension_penalty" = f64, Query, description = "Share of limits suspended servers count at"),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        Query(params): Query<Params>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(shared::ApiError::new_strings_value(errors))
                .with_status(axum::http::StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_admin_permission("nodes.read")?;

        let usages = node_recommendation::fetch_node_usages(
            &state.database,
            &params.location_uuid,
            params.suspension_penalty,
        )
        .await?;

        // `cpu` is accepted for symmetry with the deploy payload, but nodes
        // have no total CPU capacity, so only allocated CPU (relative
        // between candidates) feeds the score — see node_recommendation.
        let _ = params.cpu;

        let mut scored = node_recommendation::recommend_nodes(
            usages,
            params.memory,
            params.disk,
            params.allow_overallocation,
        );

        let best = scored.iter().position(|scored| scored.eligible());
        let (node, reason) = match best {
            Some(index) => {
                let best = scored.remove(index);
                (
                    Some(RecommendedNode::from_scored(&state, &best).await?),
                    None,
                )
            }
            None => {
                let reason = scored
                    .iter()
                    .filter_map(|scored| scored.ineligible_reason.as_ref())
                    .next()
                    .map(|reason| reason.to_string())
                    .unwrap_or_else(|| "no nodes exist".to_string());
                (None, Some(reason))
            }
        };

        let mut alternatives = Vec::new();
        for scored in scored.iter().filter(|scored| scored.eligible()).take(4) {
            alternatives.push(RecommendedNode::from_scored(&state, scored).await?);
        }

        ApiResponse::new_serialized(Response {
            node,
            reason,
            alternatives,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
