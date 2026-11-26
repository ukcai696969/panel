use serde::{Deserialize, Serialize};
use shared::models::{server::Server, server_variable::ServerVariable};
use std::path::Path;
use utoipa::ToSchema;

/// Which upstream catalogue an add-on comes from.
///
/// Modrinth is the primary source. CurseForge is a clearly-marked second
/// source: it only works when the panel process has a `CURSEFORGE_API_KEY`
/// environment variable set (see `curseforge.rs`); no key is stored in code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AddonSource {
    Modrinth,
    Curseforge,
}

impl AddonSource {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "modrinth" => Some(AddonSource::Modrinth),
            "curseforge" => Some(AddonSource::Curseforge),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AddonSource::Modrinth => "modrinth",
            AddonSource::Curseforge => "curseforge",
        }
    }
}

/// One catalogue entry (mod, plugin, modpack, datapack, shader, resource pack).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AddonProject {
    pub source: AddonSource,
    /// Modrinth project id/slug, or the CurseForge numeric mod id as a string.
    pub id: String,
    pub slug: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub icon_url: Option<String>,
    /// `mod`, `plugin`, `modpack`, `datapack`, `shader`, `resourcepack`.
    pub project_type: String,
    pub downloads: u64,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
}

/// One downloadable file belonging to a version.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AddonFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: Option<u64>,
}

/// One released version of a project.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AddonVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<AddonFile>,
    pub date_published: Option<String>,
    /// True when this version matches the server's game version / loader.
    pub compatible: bool,
}

/// What the panel knows about the server's game, derived from its egg and
/// startup variables. Minecraft eggs in the wild name these variables
/// differently (`MINECRAFT_VERSION`, `MC_VERSION`, `GAME_VERSION`, ...), so
/// detection checks a priority list instead of assuming one name.
#[derive(Debug, Clone, Default)]
pub struct ServerAddonContext {
    pub is_minecraft: bool,
    pub game_version: Option<String>,
    pub loaders: Vec<String>,
}

const GAME_VERSION_VARIABLES: &[&str] = &[
    "MINECRAFT_VERSION",
    "MC_VERSION",
    "GAME_VERSION",
    "VERSION",
];

const LOADER_VARIABLES: &[&str] = &["MOD_LOADER", "LOADER", "SERVER_TYPE", "TYPE"];

const KNOWN_LOADERS: &[&str] = &[
    "fabric", "forge", "neoforge", "quilt", "paper", "purpur", "spigot", "bukkit", "vanilla",
];

fn loaders_in(value: &str) -> Vec<String> {
    let lower = value.to_ascii_lowercase();
    KNOWN_LOADERS
        .iter()
        .filter(|loader| lower.contains(*loader))
        .map(|loader| loader.to_string())
        .collect()
}

pub fn detect_context(server: &Server, variables: &[ServerVariable]) -> ServerAddonContext {
    let mut context = ServerAddonContext::default();

    let egg_name = server.egg.name.to_ascii_lowercase();
    if egg_name.contains("minecraft") {
        context.is_minecraft = true;
    }

    for variable in variables {
        let name = variable.variable.env_variable.as_str();
        if context.game_version.is_none()
            && GAME_VERSION_VARIABLES.contains(&name)
            && !variable.value.trim().is_empty()
        {
            // Values are usually a plain version ("1.21.1"); keep only the
            // first whitespace-separated token so "1.21.1 (latest)" still works.
            let version = variable
                .value
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !version.is_empty() {
                context.game_version = Some(version);
                context.is_minecraft = true;
            }
        }
        if LOADER_VARIABLES.contains(&name) {
            for loader in loaders_in(&variable.value) {
                if !context.loaders.contains(&loader) {
                    context.loaders.push(loader);
                }
            }
        }
    }

    // A loader named in the egg (e.g. "Minecraft (Fabric)") is a fallback.
    if context.loaders.is_empty() {
        context.loaders = loaders_in(&egg_name);
    }

    context
}

/// Folder inside the server a project type is installed into.
///
/// Modpacks are the exception: a `.mrpack` is only downloaded to the server
/// root here. Extracting/installing a modpack from it is not automated by
/// this feature (it needs a pack installer on the node); the UI says so.
pub fn destination_for(project_type: &str) -> &'static str {
    match project_type {
        "mod" => "/mods",
        "plugin" => "/plugins",
        "datapack" => "/world/datapacks",
        "shader" => "/shaderpacks",
        "resourcepack" => "/resourcepacks",
        // "modpack" and anything unknown land in the server root.
        _ => "/",
    }
}

/// Reject filenames that could escape the destination folder or that are not
/// a plausible add-on artifact. Returns the cleaned file name.
pub fn sanitize_file_name(filename: &str) -> Result<String, String> {
    let name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() || name == "." || name == ".." {
        return Err("download has no usable file name".to_string());
    }
    if !name.ends_with(".jar")
        && !name.ends_with(".zip")
        && !name.ends_with(".mrpack")
        && !name.ends_with(".datapack")
    {
        return Err(format!("unsupported add-on file type: {name}"));
    }
    Ok(name)
}

/// Validate a destination override supplied by the client: absolute, inside
/// the server root, no parent-directory traversal.
pub fn validate_destination(destination: &str) -> Result<String, String> {
    if !destination.starts_with('/') {
        return Err("destination must be an absolute path".to_string());
    }
    let path = Path::new(destination);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("destination must not contain '..'".to_string());
    }
    Ok(destination.to_string())
}

const MAX_ADDON_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Allowed download hosts per source. Anything else is refused before any
/// request is made, so a crafted catalogue entry cannot point the panel (or
/// Wings) at an arbitrary host.
pub fn allowed_download_host(source: AddonSource, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    match source {
        AddonSource::Modrinth => host == "cdn.modrinth.com" || host == "api.modrinth.com",
        AddonSource::Curseforge => {
            host == "edge.forgecdn.net" || host == "media.forgecdn.net" || host.ends_with(".forgecdn.net")
        }
    }
}

/// Validate a download URL before Wings is asked to fetch it:
///
/// * https only, host on the per-source allowlist;
/// * then a HEAD request through `shared::net::outbound_request`, whose
///   resolver refuses any address covered by `APP_BLOCKED_CIDRS` and whose
///   redirect policy refuses redirects to blocked IP literals (SSRF
///   protection — see `shared/src/net.rs`).
/// * a `Content-Length` over 2 GiB is refused when the upstream sends one.
///
/// This does not download the file: the actual download is done by Wings
/// via the existing files/pull endpoint (see `install.rs`).
pub async fn validate_download_url(
    state: &shared::State,
    source: AddonSource,
    url: &str,
) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "download url is not a valid url".to_string())?;
    if parsed.scheme() != "https" {
        return Err("download url must use https".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "download url has no host".to_string())?;
    if !allowed_download_host(source, host) {
        return Err(format!("download host '{host}' is not allowed for {source:?}"));
    }

    let response = shared::net::outbound_request(&state.env, reqwest::Method::HEAD, parsed)
        .map_err(|err| format!("download url was refused: {err}"))?
        .send()
        .await
        .map_err(|err| format!("could not reach download url: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "download url returned HTTP {}",
            response.status().as_u16()
        ));
    }
    if let Some(length) = response.content_length()
        && length > MAX_ADDON_BYTES
    {
        return Err("add-on file is larger than 2 GiB".to_string());
    }

    Ok(())
}

/// Is this version usable on the server? A version with no game-version or
/// loader metadata is treated as compatible (some listings omit it); a
/// version that names versions/loaders must match at least one of each,
/// when the server actually declares them.
pub fn version_is_compatible(version: &AddonVersion, context: &ServerAddonContext) -> bool {
    if let Some(game_version) = &context.game_version
        && !version.game_versions.is_empty()
        && !version.game_versions.iter().any(|v| v == game_version)
    {
        return false;
    }
    if !context.loaders.is_empty()
        && !version.loaders.is_empty()
        && !version
            .loaders
            .iter()
            .any(|loader| context.loaders.contains(&loader.to_ascii_lowercase()))
    {
        return false;
    }
    true
}
