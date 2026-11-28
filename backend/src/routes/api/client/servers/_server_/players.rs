use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Best-effort online-player parsing for Minecraft / supported eggs.
///
/// There is no Wings endpoint that returns the player list directly, so this
/// route reuses the existing Wings console-logs endpoint
/// (`GET /api/server/{server}/logs`, already proxied by the `logs` route) and
/// parses the most recent `list` command response out of it:
///
/// ```text
/// There are 2 of a max of 20 players online: Steve, Alex
/// ```
///
/// Join/leave lines seen *after* that response (`<name> joined the game` /
/// `<name> left the game`) are applied on top, so the list stays roughly
/// current between explicit `/list` refreshes. Everything here is
/// best-effort: if no `list` output is in the recent logs the response is
/// `stale: true` with an empty player list, and the frontend tells the user
/// to press Refresh (which sends `list` through the existing `command`
/// route, then re-fetches this endpoint).
///
/// No new Wings protocol is introduced: commands still go through
/// `POST /api/server/{server}/commands` (see `command.rs`), and this route
/// only reads logs Wings already exposes.
mod get {
    use axum::http::StatusCode;
    use garde::Validate;
    use serde::{Deserialize, Serialize};
    use shared::{
        ApiError, GetState,
        models::{
            server::{GetServer, Server},
            user::GetPermissionManager,
        },
        response::{ApiResponse, ApiResponseResult},
    };
    use tokio::io::AsyncReadExt;
    use utoipa::ToSchema;

    fn default_lines() -> u64 {
        200
    }

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Params {
        #[garde(range(min = 1, max = 1000))]
        #[serde(default = "default_lines")]
        pub lines: u64,
    }

    #[derive(ToSchema, Serialize)]
    pub struct Response {
        pub players: Vec<compact_str::CompactString>,
        pub online: usize,
        pub max: Option<u64>,
        pub supported: bool,
        pub source_line: Option<compact_str::CompactString>,
        pub stale: bool,
        pub fetched_at: chrono::DateTime<chrono::Utc>,
    }

    /// Egg/nest names that identify a Minecraft-like server. This is a
    /// heuristic, not a registry: eggs are user-created, so a renamed or
    /// custom egg can slip through (it then simply reports `supported:
    /// false` until its name matches, or `stale` with no `list` output).
    const MINECRAFT_HINTS: &[&str] = &[
        "minecraft",
        "paper",
        "purpur",
        "spigot",
        "bukkit",
        "fabric",
        "forge",
        "neoforge",
        "folia",
        "bungeecord",
        "waterfall",
        "velocity",
    ];

    pub fn is_minecraft_like(server: &Server) -> bool {
        let haystack = format!("{} {}", server.egg.name, server.nest.name).to_lowercase();

        MINECRAFT_HINTS.iter().any(|hint| haystack.contains(hint))
    }

    /// Strips ANSI escape sequences (colour codes etc.) so log lines can be
    /// matched as plain text. Hand-rolled: no extra dependency for this.
    pub fn strip_ansi_codes(line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();

        while let Some(char) = chars.next() {
            if char != '\x1b' {
                out.push(char);
                continue;
            }

            match chars.peek() {
                // CSI sequence: ESC [ ... final byte in @..~
                Some('[') => {
                    chars.next();
                    for char in chars.by_ref() {
                        if ('@'..='~').contains(&char) {
                            break;
                        }
                    }
                }
                // OSC sequence: ESC ] ... terminated by BEL or ESC \
                Some(']') => {
                    chars.next();
                    let mut previous = '\0';
                    for char in chars.by_ref() {
                        if char == '\x07' || (previous == '\x1b' && char == '\\') {
                            break;
                        }
                        previous = char;
                    }
                }
                // Any other escape: drop the ESC and the next byte.
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        }

        out
    }

    fn is_valid_player_name(name: &str) -> bool {
        (3..=16).contains(&name.chars().count())
            && name
                .chars()
                .all(|char| char.is_ascii_alphanumeric() || char == '_')
    }

    /// Parses one `There are X of a max of Y players online: ...` line.
    /// Returns `(players, online, max)`. `None` when the line is not a
    /// `list` response.
    pub fn parse_list_line(line: &str) -> Option<(Vec<compact_str::CompactString>, u64, u64)> {
        let plain = strip_ansi_codes(line);
        let (_, rest) = plain.split_once("There are ")?;
        let (online, rest) = rest.split_once(" of a max of ")?;
        let (max, rest) = rest.split_once(" players online")?;
        let online: u64 = online.trim().parse().ok()?;
        let max: u64 = max.trim().parse().ok()?;

        let names = rest
            .trim_start_matches(':')
            .split(',')
            .map(str::trim)
            .filter(|name| is_valid_player_name(name))
            .map(compact_str::CompactString::from)
            .collect();

        Some((names, online, max))
    }

    /// Applies a join/leave line seen after a `list` response, returning the
    /// updated player list, or `None` when the line is neither.
    fn apply_join_leave_line(
        players: &[compact_str::CompactString],
        line: &str,
    ) -> Option<Vec<compact_str::CompactString>> {
        let plain = strip_ansi_codes(line);
        let plain = plain.trim_end();
        let (name, action) = plain
            .rsplit_once(" joined the game")
            .map(|(name, _)| (name, true))
            .or_else(|| {
                plain
                    .rsplit_once(" left the game")
                    .map(|(name, _)| (name, false))
            })?;
        let name = name.rsplit([' ', ']', ':']).next().unwrap_or(name);
        let name = name.trim();
        if !is_valid_player_name(name) {
            return None;
        }

        let mut players: Vec<compact_str::CompactString> = players.to_vec();
        if action {
            if !players.iter().any(|player| player.as_str() == name) {
                players.push(name.into());
            }
        } else {
            players.retain(|player| player.as_str() != name);
        }

        Some(players)
    }

    pub struct ParsedPlayers {
        pub players: Vec<compact_str::CompactString>,
        pub online: usize,
        pub max: Option<u64>,
        pub source_line: Option<compact_str::CompactString>,
    }

    /// Parses raw console lines (oldest first). The last `list` response
    /// wins; join/leave lines after it adjust the list. When no `list`
    /// response is present the list is empty and `source_line` is `None`
    /// (the caller reports that as `stale`).
    pub fn parse_players_from_lines(lines: &[&str]) -> ParsedPlayers {
        let mut parsed = ParsedPlayers {
            players: Vec::new(),
            online: 0,
            max: None,
            source_line: None,
        };

        for line in lines {
            if let Some((players, online, max)) = parse_list_line(line) {
                parsed.players = players;
                parsed.online = online as usize;
                parsed.max = Some(max);
                parsed.source_line = Some(strip_ansi_codes(line).trim().into());
            } else if parsed.source_line.is_some()
                && let Some(players) = apply_join_leave_line(&parsed.players, line)
            {
                parsed.players = players;
                parsed.online = parsed.players.len();
            }
        }

        parsed
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = UNAUTHORIZED, body = ApiError),
    ), params(
        (
            "server" = uuid::Uuid,
            description = "The server ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "lines" = i64, Query,
            description = "How many recent console lines to parse for a list response",
            example = "200",
            minimum = 1,
            maximum = 1000,
        ),
    ))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        server: GetServer,
        axum::extract::Query(params): axum::extract::Query<Params>,
    ) -> ApiResponseResult {
        if let Err(errors) = shared::utils::validate_data(&params) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        permissions.has_server_permission("control.read-console")?;

        let supported = is_minecraft_like(&server.0);
        if !supported {
            return ApiResponse::new_serialized(Response {
                players: Vec::new(),
                online: 0,
                max: None,
                supported: false,
                source_line: None,
                stale: true,
                fetched_at: chrono::Utc::now(),
            })
            .ok();
        }

        let logs = server
            .node
            .fetch_cached(&state.database)
            .await?
            .api_client(&state.database)
            .await?
            .get_servers_server_logs(
                server.uuid,
                &wings_api::servers_server_logs::get::Query {
                    lines: Some(params.lines),
                    ..Default::default()
                },
            )
            .await?;

        // The Wings logs call returns an AsyncRead (the same value the
        // `logs` route streams straight through). Pin it so it can be read
        // to a string without assuming anything about its concrete type.
        let mut logs = Box::pin(logs);
        let mut text = String::new();
        logs.read_to_string(&mut text).await.map_err(|err| {
            ApiResponse::error(format!("failed to read server logs: {err}"))
                .with_status(StatusCode::BAD_GATEWAY)
        })?;

        let lines: Vec<&str> = text.lines().collect();
        let parsed = parse_players_from_lines(&lines);

        ApiResponse::new_serialized(Response {
            online: parsed.online,
            max: parsed.max,
            stale: parsed.source_line.is_none(),
            source_line: parsed.source_line,
            players: parsed.players,
            supported: true,
            fetched_at: chrono::Utc::now(),
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}

#[cfg(test)]
mod tests {
    use super::get::{parse_list_line, parse_players_from_lines, strip_ansi_codes};

    #[test]
    fn parses_a_vanilla_list_response() {
        let (players, online, max) =
            parse_list_line("[12:00:00 INFO]: There are 2 of a max of 20 players online: Steve, Alex")
                .unwrap();

        assert_eq!(online, 2);
        assert_eq!(max, 20);
        assert_eq!(players, vec!["Steve", "Alex"]);
    }

    #[test]
    fn parses_an_empty_list_response() {
        let (players, online, max) =
            parse_list_line("There are 0 of a max of 20 players online:").unwrap();

        assert!(players.is_empty());
        assert_eq!(online, 0);
        assert_eq!(max, 20);
    }

    #[test]
    fn ignores_non_list_lines() {
        assert!(parse_list_line("[12:00:00 INFO]: Steve joined the game").is_none());
        assert!(parse_list_line("There are spiders nearby").is_none());
    }

    #[test]
    fn strips_ansi_codes_before_parsing() {
        assert_eq!(
            strip_ansi_codes("\x1b[32mSteve\x1b[0m joined"),
            "Steve joined"
        );
        let (players, _, _) =
            parse_list_line("\x1b[33mThere are 1 of a max of 20 players online: Steve\x1b[0m")
                .unwrap();
        assert_eq!(players, vec!["Steve"]);
    }

    #[test]
    fn applies_joins_and_leaves_after_the_list_response() {
        let parsed = parse_players_from_lines(&[
            "[12:00:00 INFO]: There are 1 of a max of 20 players online: Steve",
            "[12:01:00 INFO]: Alex joined the game",
            "[12:02:00 INFO]: Steve left the game",
        ]);

        assert_eq!(parsed.players, vec!["Alex"]);
        assert_eq!(parsed.online, 1);
        assert!(parsed.source_line.is_some());
    }

    #[test]
    fn ignores_joins_before_any_list_response() {
        let parsed = parse_players_from_lines(&["[12:01:00 INFO]: Alex joined the game"]);

        assert!(parsed.players.is_empty());
        assert!(parsed.source_line.is_none());
    }

    #[test]
    fn the_last_list_response_wins() {
        let parsed = parse_players_from_lines(&[
            "There are 1 of a max of 20 players online: Steve",
            "There are 2 of a max of 20 players online: Steve, Alex",
        ]);

        assert_eq!(parsed.players, vec!["Steve", "Alex"]);
    }
}
