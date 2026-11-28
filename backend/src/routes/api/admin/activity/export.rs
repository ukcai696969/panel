//! `GET /api/admin/activity/export` — export activity logs as CSV or JSON.
//!
//! Query parameters:
//! - `format=csv|json` (default `csv`)
//! - `from` / `to` — RFC 3339 datetimes, inclusive bounds on `created`
//! - `type=admin|user|team|server|all` (default `admin`)
//!
//! Admin-only (`activity.read`). At most [`MAX_EXPORT_ROWS`] rows are
//! returned, newest first; the export is a point-in-time snapshot, not a
//! stream, so very large exports are capped rather than timing out.

use axum::{body::Body, extract::Query, http::StatusCode};
use serde::{Deserialize, Serialize};
use shared::{
    ApiError, GetState,
    models::{admin_activity::GetAdminActivityLogger, user::GetPermissionManager},
    response::{ApiResponse, ApiResponseResult},
};
use sqlx::Row;
use utoipa::ToSchema;

const MAX_EXPORT_ROWS: i64 = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Csv,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActivityExportType {
    #[default]
    Admin,
    User,
    Team,
    Server,
    All,
}

impl ActivityExportType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
            Self::Team => "team",
            Self::Server => "server",
            Self::All => "all",
        }
    }
}

#[derive(ToSchema, Deserialize)]
pub struct Params {
    #[serde(default)]
    format: ExportFormat,
    from: Option<chrono::DateTime<chrono::Utc>>,
    to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, rename = "type")]
    activity_type: ActivityExportType,
}

/// One row of the export, with the same columns for every activity kind so
/// CSV and JSON output share a shape.
#[derive(Serialize)]
struct ExportRow {
    #[serde(rename = "type")]
    kind: &'static str,
    created: chrono::DateTime<chrono::Utc>,
    event: String,
    username: Option<String>,
    user_uuid: Option<uuid::Uuid>,
    ip: Option<String>,
    is_api: bool,
    server_uuid: Option<uuid::Uuid>,
    team_uuid: Option<uuid::Uuid>,
    data: serde_json::Value,
}

async fn fetch_rows(
    database: &shared::database::Database,
    kind: ActivityExportType,
    from: Option<chrono::NaiveDateTime>,
    to: Option<chrono::NaiveDateTime>,
) -> Result<Vec<ExportRow>, shared::database::DatabaseError> {
    // Each activity table has the same core columns; the joins only resolve
    // the actor's username. `NULL::uuid` keeps the column list identical so
    // all four kinds map through one `ExportRow`.
    let (table, subject_column) = match kind {
        ActivityExportType::Admin => ("admin_activities", "NULL::uuid AS server_uuid, NULL::uuid AS team_uuid"),
        ActivityExportType::User => ("user_activities", "NULL::uuid AS server_uuid, NULL::uuid AS team_uuid"),
        ActivityExportType::Team => ("team_activities", "NULL::uuid AS server_uuid, team_activities.team_uuid AS team_uuid"),
        ActivityExportType::Server => ("server_activities", "server_activities.server_uuid AS server_uuid, NULL::uuid AS team_uuid"),
        ActivityExportType::All => unreachable!("`all` fans out to the four kinds"),
    };

    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {table}.created AS created,
            {table}.event AS event,
            {table}.ip AS ip,
            {table}.data AS data,
            ({table}.api_key_uuid IS NOT NULL) AS is_api,
            users.username AS username,
            users.uuid AS user_uuid,
            {subject_column}
        FROM {table}
        LEFT JOIN users ON users.uuid = {table}.user_uuid
        WHERE ($1::timestamp IS NULL OR {table}.created >= $1)
            AND ($2::timestamp IS NULL OR {table}.created <= $2)
        ORDER BY {table}.created DESC, {table}.ctid DESC
        LIMIT $3
        "#
    )))
    .bind(from)
    .bind(to)
    .bind(MAX_EXPORT_ROWS)
    .fetch_all(database.read())
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ExportRow {
                kind: kind.as_str(),
                created: row
                    .try_get::<chrono::NaiveDateTime, _>("created")?
                    .and_utc(),
                event: row.try_get("event")?,
                username: row.try_get("username")?,
                user_uuid: row.try_get("user_uuid")?,
                ip: row
                    .try_get::<Option<sqlx::types::ipnetwork::IpNetwork>, _>("ip")?
                    .map(|ip| ip.ip().to_string()),
                is_api: row.try_get("is_api")?,
                server_uuid: row.try_get("server_uuid")?,
                team_uuid: row.try_get("team_uuid")?,
                data: row.try_get("data")?,
            })
        })
        .collect::<Result<Vec<_>, shared::database::DatabaseError>>()
}

/// Appends one CSV field, quoting (and doubling embedded quotes) whenever
/// the value contains a comma, a quote, or a line break (RFC 4180).
fn push_csv_field(out: &mut String, value: &str) {
    if value.contains([',', '"', '\n', '\r']) {
        out.push('"');
        for c in value.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(value);
    }
}

fn rows_to_csv(rows: &[ExportRow]) -> String {
    let mut out = String::from(
        "created,type,event,username,user_uuid,ip,is_api,server_uuid,team_uuid,data\n",
    );

    for row in rows {
        let mut line = String::new();
        push_csv_field(&mut line, &row.created.to_rfc3339());
        line.push(',');
        push_csv_field(&mut line, row.kind);
        line.push(',');
        push_csv_field(&mut line, &row.event);
        line.push(',');
        push_csv_field(&mut line, row.username.as_deref().unwrap_or_default());
        line.push(',');
        push_csv_field(
            &mut line,
            &row.user_uuid.map(|uuid| uuid.to_string()).unwrap_or_default(),
        );
        line.push(',');
        push_csv_field(&mut line, row.ip.as_deref().unwrap_or_default());
        line.push(',');
        push_csv_field(&mut line, if row.is_api { "true" } else { "false" });
        line.push(',');
        push_csv_field(
            &mut line,
            &row.server_uuid
                .map(|uuid| uuid.to_string())
                .unwrap_or_default(),
        );
        line.push(',');
        push_csv_field(
            &mut line,
            &row.team_uuid.map(|uuid| uuid.to_string()).unwrap_or_default(),
        );
        line.push(',');
        push_csv_field(&mut line, &row.data.to_string());
        line.push('\n');

        out.push_str(&line);
    }

    out
}

#[utoipa::path(get, path = "/export", responses(
    (status = OK, description = "CSV or JSON file download"),
    (status = BAD_REQUEST, body = ApiError),
), params(
    (
        "format" = Option<String>, Query,
        description = "Export format: csv or json (default csv)",
        example = "csv",
    ),
    (
        "from" = Option<String>, Query,
        description = "Only include activity created at or after this RFC 3339 datetime",
        example = "2026-11-01T00:00:00Z",
    ),
    (
        "to" = Option<String>, Query,
        description = "Only include activity created at or before this RFC 3339 datetime",
        example = "2026-11-26T00:00:00Z",
    ),
    (
        "type" = Option<String>, Query,
        description = "Which activity log to export: admin, user, team, server, or all (default admin)",
        example = "admin",
    ),
))]
pub async fn route(
    state: GetState,
    permissions: GetPermissionManager,
    activity_logger: GetAdminActivityLogger,
    Query(params): Query<Params>,
) -> ApiResponseResult {
    permissions.has_admin_permission("activity.read")?;

    if let (Some(from), Some(to)) = (params.from, params.to)
        && from > to
    {
        return ApiResponse::new_serialized(ApiError::new_value(&[
            "`from` must not be after `to`",
        ]))
        .with_status(StatusCode::BAD_REQUEST)
        .ok();
    }

    let from = params.from.map(|dt| dt.naive_utc());
    let to = params.to.map(|dt| dt.naive_utc());

    let mut rows = if params.activity_type == ActivityExportType::All {
        let mut rows = Vec::new();
        for kind in [
            ActivityExportType::Admin,
            ActivityExportType::User,
            ActivityExportType::Team,
            ActivityExportType::Server,
        ] {
            rows.extend(fetch_rows(&state.database, kind, from, to).await?);
        }
        rows.sort_by(|a, b| b.created.cmp(&a.created));
        rows.truncate(MAX_EXPORT_ROWS as usize);
        rows
    } else {
        fetch_rows(&state.database, params.activity_type, from, to).await?
    };

    // `fetch_rows` returns newest-first per kind; keep that order for the
    // single-kind case and for the merged `all` case above.
    rows.sort_by(|a, b| b.created.cmp(&a.created));

    let row_count = rows.len();

    let (body, content_type, extension) = match params.format {
        ExportFormat::Csv => (
            Body::from(rows_to_csv(&rows)),
            "text/csv; charset=utf-8",
            "csv",
        ),
        ExportFormat::Json => (
            Body::from(serde_json::to_vec(&rows).map_err(anyhow::Error::new)?),
            "application/json",
            "json",
        ),
    };

    let filename = format!(
        "activity-export-{}.{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        extension
    );

    activity_logger
        .log(
            "activity:export",
            serde_json::json!({
                "format": match params.format {
                    ExportFormat::Csv => "csv",
                    ExportFormat::Json => "json",
                },
                "type": params.activity_type.as_str(),
                "from": params.from,
                "to": params.to,
                "rows": row_count,
            }),
        )
        .await;

    ApiResponse::new(body)
        .with_header("Content-Type", content_type)
        .with_header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escaping_quotes_fields_with_commas_quotes_and_newlines() {
        let mut out = String::new();
        push_csv_field(&mut out, "plain");
        assert_eq!(out, "plain");

        let mut out = String::new();
        push_csv_field(&mut out, "has,comma");
        assert_eq!(out, "\"has,comma\"");

        let mut out = String::new();
        push_csv_field(&mut out, "say \"hi\"\nbye");
        assert_eq!(out, "\"say \"\"hi\"\"\nbye\"");
    }
}
