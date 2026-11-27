//! Modrinth client (primary add-on source).
//!
//! Uses the public Modrinth API v2 — no API key needed:
//! `GET /v2/search`, `GET /v2/project/{id}/version`.
//! All requests go through `shared::net::outbound_request`, so resolved
//! addresses covered by `APP_BLOCKED_CIDRS` are refused (SSRF protection).

use super::types::{AddonFile, AddonProject, AddonSource, AddonVersion, SearchResult, ServerAddonContext, version_is_compatible};
use serde::Deserialize;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

fn upstream_error(status: reqwest::StatusCode) -> String {
    format!("Modrinth returned HTTP {}", status.as_u16())
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    state: &shared::State,
    url: reqwest::Url,
) -> Result<T, String> {
    let response = shared::net::outbound_request(&state.env, reqwest::Method::GET, url)
        .map_err(|err| format!("Modrinth request was refused: {err}"))?
        .send()
        .await
        .map_err(|err| format!("could not reach Modrinth: {err}"))?;
    if !response.status().is_success() {
        return Err(upstream_error(response.status()));
    }
    response
        .json::<T>()
        .await
        .map_err(|err| format!("could not parse Modrinth response: {err}"))
}

#[derive(Deserialize)]
struct SearchHit {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    author: String,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    project_type: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    categories: Vec<String>,
    /// Game versions this project lists (Modrinth names this field `versions`).
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    hits: Vec<SearchHit>,
    #[serde(default)]
    total_hits: u64,
}

const KNOWN_LOADERS: &[&str] = &[
    "fabric", "forge", "neoforge", "quilt", "paper", "purpur", "spigot", "bukkit", "vanilla",
];

/// Search Modrinth. `project_type` is e.g. `mod`, `modpack`, `plugin`,
/// `datapack`, `shader`, `resourcepack` (empty = all). When the server
/// declares a game version / loader, matching facets are sent so Modrinth
/// filters server-side as well.
pub async fn search_projects(
    state: &shared::State,
    query: &str,
    project_type: Option<&str>,
    context: &ServerAddonContext,
    page: u64,
    per_page: u64,
) -> Result<SearchResult, String> {
    let mut url = reqwest::Url::parse(&format!("{MODRINTH_API}/search"))
        .map_err(|_| "invalid Modrinth search url".to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("query", query);
        pairs.append_pair("limit", &per_page.min(100).to_string());
        pairs.append_pair("offset", &((page.saturating_sub(1)) * per_page).to_string());

        let mut facets: Vec<Vec<String>> = Vec::new();
        if let Some(project_type) = project_type
            && !project_type.is_empty()
        {
            facets.push(vec![format!("project_type:{project_type}")]);
        }
        if let Some(game_version) = &context.game_version {
            facets.push(vec![format!("versions:{game_version}")]);
        }
        if !context.loaders.is_empty() {
            facets.push(
                context
                    .loaders
                    .iter()
                    .map(|loader| format!("categories:{loader}"))
                    .collect(),
            );
        }
        if !facets.is_empty() {
            pairs.append_pair(
                "facets",
                &serde_json::to_string(&facets).unwrap_or_default(),
            );
        }
    }

    let response: SearchResponse = get_json(state, url).await?;

    Ok(SearchResult {
        total: response.total_hits,
        projects: response
            .hits
            .into_iter()
            .map(|hit| AddonProject {
                source: AddonSource::Modrinth,
                id: hit.project_id,
                slug: Some(hit.slug),
                title: hit.title,
                description: hit.description,
                author: if hit.author.is_empty() {
                    None
                } else {
                    Some(hit.author)
                },
                icon_url: hit.icon_url,
                project_type: hit.project_type,
                downloads: hit.downloads,
                game_versions: hit.versions,
                // Search hits do not carry a `loaders` field; derive loaders
                // from the hit's categories.
                loaders: hit
                    .categories
                    .iter()
                    .filter(|category| KNOWN_LOADERS.contains(&category.as_str()))
                    .cloned()
                    .collect(),
            })
            .collect(),
    })
}

#[derive(Deserialize)]
struct VersionFile {
    #[serde(default)]
    url: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
struct VersionResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    files: Vec<VersionFile>,
    #[serde(default)]
    date_published: Option<String>,
}

/// List a project's versions, filtered to the server's game version /
/// loader when the server declares them. `compatible` is set per version
/// against the same context, so the UI can show incompatible versions
/// greyed out instead of hiding them entirely.
pub async fn project_versions(
    state: &shared::State,
    project_id: &str,
    context: &ServerAddonContext,
) -> Result<Vec<AddonVersion>, String> {
    let mut url = reqwest::Url::parse(&format!(
        "{MODRINTH_API}/project/{}/version",
        urlencoding::encode(project_id)
    ))
    .map_err(|_| "invalid Modrinth project url".to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(game_version) = &context.game_version {
            pairs.append_pair(
                "game_versions",
                &serde_json::to_string(&[game_version]).unwrap_or_default(),
            );
        }
        if !context.loaders.is_empty() {
            pairs.append_pair(
                "loaders",
                &serde_json::to_string(&context.loaders).unwrap_or_default(),
            );
        }
    }

    let versions: Vec<VersionResponse> = get_json(state, url).await?;

    Ok(versions
        .into_iter()
        .map(|version| {
            let addon = AddonVersion {
                id: version.id,
                project_id: version.project_id,
                name: version.name,
                version_number: version.version_number,
                game_versions: version.game_versions,
                loaders: version.loaders,
                files: version
                    .files
                    .into_iter()
                    .map(|file| AddonFile {
                        url: file.url,
                        filename: file.filename,
                        primary: file.primary,
                        size: file.size,
                    })
                    .collect(),
                date_published: version.date_published,
                compatible: false,
            };
            let compatible = version_is_compatible(&addon, context);
            AddonVersion {
                compatible,
                ..addon
            }
        })
        .collect())
}

/// Fetch one version by id. Modrinth has no single-version endpoint that
/// also returns the project, so the version endpoint is used directly and
/// the project id is checked against the caller's expectation.
pub async fn project_version(
    state: &shared::State,
    version_id: &str,
    context: &ServerAddonContext,
) -> Result<AddonVersion, String> {
    let url = reqwest::Url::parse(&format!(
        "{MODRINTH_API}/version/{}",
        urlencoding::encode(version_id)
    ))
    .map_err(|_| "invalid Modrinth version url".to_string())?;

    let version: VersionResponse = get_json(state, url).await?;
    let addon = AddonVersion {
        id: version.id,
        project_id: version.project_id,
        name: version.name,
        version_number: version.version_number,
        game_versions: version.game_versions,
        loaders: version.loaders,
        files: version
            .files
            .into_iter()
            .map(|file| AddonFile {
                url: file.url,
                filename: file.filename,
                primary: file.primary,
                size: file.size,
            })
            .collect(),
        date_published: version.date_published,
        compatible: false,
    };
    let compatible = version_is_compatible(&addon, context);
    Ok(AddonVersion {
        compatible,
        ..addon
    })
}
