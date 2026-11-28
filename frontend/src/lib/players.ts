/**
 * Helpers for the online-players panel and the console error filter.
 *
 * The player list itself comes from the backend (`GET
 * /api/client/servers/{server}/players`), which parses the server's recent
 * console output. The functions here are the frontend mirror of that
 * parsing, used to apply live console lines (join/leave, and a fresh `list`
 * response) to the displayed list without waiting for the next poll, plus
 * the shared Minecraft heuristics and error-line detection.
 */

/** Egg/nest name fragments that identify a Minecraft-like server. */
const MINECRAFT_HINTS = [
  'minecraft',
  'paper',
  'purpur',
  'spigot',
  'bukkit',
  'fabric',
  'forge',
  'neoforge',
  'folia',
  'bungeecord',
  'waterfall',
  'velocity',
] as const;

export function isMinecraftLikeServer(server: {
  egg: { name: string; description: string | null };
}): boolean {
  const haystack = `${server.egg.name} ${server.egg.description ?? ''}`.toLowerCase();
  return MINECRAFT_HINTS.some((hint) => haystack.includes(hint));
}

/** A Minecraft username: 3-16 ASCII letters, digits or underscores. */
export const MINECRAFT_PLAYER_NAME = /^[A-Za-z0-9_]{3,16}$/;

/** A Minecraft namespaced item id, e.g. `minecraft:diamond_sword`. */
export const MINECRAFT_ITEM_ID = /^[a-z0-9_:]+$/;

const ANSI_CSI = /\x1b\[[0-9;?]*[ -/]*[@-~]/g;
const ANSI_OSC = /\x1b\][^\x07\x1b]*(\x07|\x1b\\)/g;

/** Strips ANSI colour/formatting escapes so a console line can be matched. */
export function stripAnsiCodes(line: string): string {
  return line.replace(ANSI_CSI, '').replace(ANSI_OSC, '');
}

/**
 * Matches lines that report an error: an explicit ERROR/SEVERE/FATAL level,
 * a Java-style `*Exception`, or a stack-trace frame (`at com.example...`).
 * Used both to tint live console lines red and for the "errors only" filter.
 */
const ERROR_LINE_PATTERN = /\b(ERROR|SEVERE|FATAL|CRITICAL)\b|Exception\b|^\s*at\s+[\w.$]+\(/;

export function isErrorConsoleLine(line: string): boolean {
  return ERROR_LINE_PATTERN.test(stripAnsiCodes(line));
}

/**
 * Parses one `There are X of a max of Y players online: ...` line.
 * Returns `null` when the line is not a `list` response.
 */
export function parseMinecraftListLine(line: string): { players: string[]; online: number; max: number } | null {
  const plain = stripAnsiCodes(line);
  const listIndex = plain.indexOf('There are ');
  if (listIndex === -1) return null;

  const match = /There are (\d+) of a max of (\d+) players online(?::\s*(.*?))?\s*$/.exec(
    plain.slice(listIndex),
  );
  if (!match) return null;

  const players = (match[3] ?? '')
    .split(',')
    .map((name) => name.trim())
    .filter((name) => MINECRAFT_PLAYER_NAME.test(name));

  return { players, online: Number(match[1]), max: Number(match[2]) };
}

/**
 * Applies one live console line to a player list: a fresh `list` response
 * replaces it, a join adds a name, a leave removes one. Returns the new list,
 * or `null` when the line changes nothing.
 */
export function applyConsoleLine(players: string[], line: string): string[] | null {
  const list = parseMinecraftListLine(line);
  if (list) return list.players;

  const plain = stripAnsiCodes(line).trimEnd();
  const joined = /([A-Za-z0-9_]{3,16}) joined the game\s*$/.exec(plain);
  if (joined) {
    return players.includes(joined[1]) ? null : [...players, joined[1]];
  }

  const left = /([A-Za-z0-9_]{3,16}) left the game\s*$/.exec(plain);
  if (left) {
    return players.includes(left[1]) ? players.filter((player) => player !== left[1]) : null;
  }

  return null;
}
