//! CurseForge client — clearly-marked SECOND source.
//!
//! Modrinth (see `modrinth.rs`) is the primary source and needs no key.
//! CurseForge needs an API key: set `CURSEFORGE_API_KEY` in the panel
//! process environment. No key is stored in code or in the database.
//! When the variable is absent, every CurseForge request fails with a
//! clear 503-style error (`curseforge_not_configured`) instead of
//! silently falling back to anything else.
//!
//! CurseForge API: `GET /v1/mods/search`, `GET /v1/mods/{id}/files`,
//! `GET /v1/mods/{id}/files/{fileId}`. Game id 432 is Minecraft.
//! Only mod / modpack / plugin are mapped here; CurseForge class ids for
//! shaders and resource packs are not mapped, so those types are
//! Modrinth-only for now.

use super::types::{AddonFile, AddonProject, AddonSource, AddonVersion, SearchResult, ServerAddonContext, version_is_compatible};
use serde::Deserialize;

const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";
const MINECRAFT_GAME_ID: u32 = 432;

/// CurseForge class ids (see https://docs.curseforge.com/rest-api/).
fn class_id(project_type: &str) -> Option<u32> {
    match project_type {
        "mod" => Some(6),
        "modpack" => Some(4471),
        "plugin" => Some(5),
        _ => None,
    }
}

fn project_type_from_class(class_id: u32) -> &'static str {
    match class_id {
        6 => "mod",
        4471 => "modpack",
        5 => "plugin",
        _ => "mod",
    }
}

/// CurseForge mod-loader ids.
fn loader_id(loader: &str) -> Option<u32> {
    match loader {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

fn api_key() -> Result<String, String> {
    std::env::var("CURSEFORGE_API_KEY")
        .map_err(|_| {
            "CurseForge is not configured: set CURSEFORGE_API_KEY on the panel to enable it"
                .to_string()
        })
        .and_then(|key| {
            if key.trim().is_empty() {
                Err("CurseForge is not configured: CURSEFORGE_API_KEY is empty".to_string())
            } else {
                Ok(key)
            }
        })
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    state: &shared::State,
    url: reqwest::Url,
) -> Result<T, String> {
    let key = api_key()?;
    let response = shared::net::outbound_request(&state.env, reqwest::Method::GET, url)
        .map_err(|err| format!("CurseForge request was refused: {err}"))?
        .header("x-api-key", key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("could not reach CurseForge: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge returned HTTP {}",
            response.status().as_u16()
        ));
    }
    response
        .json::<T>()
        .await
        .map_err(|err| format!("could not parse CurseForge response: {err}"))
}

#[derive(Deserialize)]
struct CfAuthor {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct CfLogo {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct CfMod {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    authors: Vec<CfAuthor>,
    #[serde(default)]
    logo: Option<CfLogo>,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    class_id: u32,
}

#[derive(Deserialize)]
struct CfSearchResponse {
    #[serde(default)]
    data: Vec<CfMod>,
    #[serde(default)]
    pagination: Option<CfPagination>,
}

#[derive(Deserialize)]
struct CfPagination {
    #[serde(default)]
    total_count: u64,
}

pub async fn search_projects(
    state: &shared::State,
    query: &str,
    project_type: Option<&str>,
    context: &ServerAddonContext,
    page: u64,
    per_page: u64,
) -> Result<SearchResult, String> {
    let mut url = reqwest::Url::parse(&format!("{CURSEFORGE_API}/mods/search"))
        .map_err(|_| "invalid CurseForge search url".to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("gameId", &MINECRAFT_GAME_ID.to_string());
        pairs.append_pair("searchFilter", query);
        pairs.append_pair("pageSize", &per_page.min(50).to_string());
        pairs.append_pair("index", &((page.saturating_sub(1)) * per_page).to_string());
        if let Some(project_type) = project_type
            && let Some(class) = class_id(project_type)
        {
            pairs.append_pair("classId", &class.to_string());
        }
        if let Some(game_version) = &context.game_version {
            pairs.append_pair("gameVersion", game_version);
        }
        // CurseForge takes a single loader id; use the first known one.
        if let Some(loader) = context.loaders.iter().find_map(|l| loader_id(l)) {
            pairs.append_pair("modLoaderType", &loader.to_string());
        }
    }

    let response: CfSearchResponse = get_json(state, url).await?;

    Ok(SearchResult {
        total: response.pagination.map(|p| p.total_count).unwrap_or(
            response.data.len() as u64,
        ),
        projects: response
            .data
            .into_iter()
            .map(|mod_| AddonProject {
                source: AddonSource::Curseforge,
                id: mod_.id.to_string(),
                slug: Some(mod_.slug),
                title: mod_.name,
                description: mod_.summary,
                author: mod_
                    .authors
                    .first()
                    .map(|author| author.name.clone())
                    .filter(|name| !name.is_empty()),
                icon_url: mod_.logo.and_then(|logo| logo.url),
                project_type: project_type_from_class(mod_.class_id).to_string(),
                downloads: mod_.download_count,
                game_versions: context
                    .game_version
                    .clone()
                    .into_iter()
                    .collect(),
                loaders: context.loaders.clone(),
            })
            .collect(),
    })
}

#[derive(Deserialize)]
struct CfFile {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    file_date: Option<String>,
    #[serde(default)]
    file_length: Option<u64>,
    #[serde(default)]
    game_versions: Vec<String>,
}

#[derive(Deserialize)]
struct CfFilesResponse {
    #[serde(default)]
    data: Vec<CfFile>,
}

#[derive(Deserialize)]
struct CfFileResponse {
    #[serde(default)]
    data: CfFile,
}

fn to_version(file: CfFile, project_id: &str, context: &ServerAddonContext) -> AddonVersion {
    let loaders = file
        .game_versions
        .iter()
        .filter(|version| {
            ["fabric", "forge", "neoforge", "quilt"]
                .contains(&version.to_ascii_lowercase().as_str())
        })
        .map(|version| version.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let game_versions = file
        .game_versions
        .iter()
        .filter(|version| !loaders.contains(&version.to_ascii_lowercase()))
        .cloned()
        .collect::<Vec<_>>();

    let addon = AddonVersion {
        id: file.id.to_string(),
        project_id: project_id.to_string(),
        name: file.display_name,
        version_number: file.file_name.clone(),
        game_versions,
        loaders,
        files: vec![AddonFile {
            // CurseForge files without a download url cannot be installed
            // by Wings; they are filtered out by the caller.
            url: file.download_url.unwrap_or_default(),
            filename: file.file_name,
            primary: true,
            size: file.file_length,
        }],
        date_published: file.file_date,
        compatible: false,
    };
    let compatible = version_is_compatible(&addon, context);
    AddonVersion {
        compatible,
        ..addon
    }
}

pub async fn project_versions(
    state: &shared::State,
    project_id: &str,
    context: &ServerAddonContext,
) -> Result<Vec<AddonVersion>, String> {
    let mut url = reqwest::Url::parse(&format!(
        "{CURSEFORGE_API}/mods/{}/files",
        urlencoding::encode(project_id)
    ))
    .map_err(|_| "invalid CurseForge files url".to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("pageSize", "25");
        if let Some(game_version) = &context.game_version {
            pairs.append_pair("gameVersion", game_version);
        }
        if let Some(loader) = context.loaders.iter().find_map(|l| loader_id(l)) {
            pairs.append_pair("modLoaderType", &loader.to_string());
        }
    }

    let response: CfFilesResponse = get_json(state, url).await?;

    Ok(response
        .data
        .into_iter()
        .map(|file| to_version(file, project_id, context))
        .filter(|version| {
            version
                .files
                .iter()
                .any(|file| !file.url.is_empty())
        })
        .collect())
}

pub async fn project_version(
    state: &shared::State,
    project_id: &str,
    version_id: &str,
    context: &ServerAddonContext,
) -> Result<AddonVersion, String> {
    let url = reqwest::Url::parse(&format!(
        "{CURSEFORGE_API}/mods/{}/files/{}",
        urlencoding::encode(project_id),
        urlencoding::encode(version_id)
    ))
    .map_err(|_| "invalid CurseForge file url".to_string())?;

    let response: CfFileResponse = get_json(state, url).await?;
    Ok(to_version(response.data, project_id, context))
}
