import { z } from 'zod';

/**
 * Response of `GET /api/client/servers/{server}/players`.
 *
 * `stale` is true when no `list` output was found in the recent console
 * logs (or the egg is not Minecraft-like, in which case `supported` is
 * false): the list is then empty and the UI should offer a Refresh, which
 * sends `list` to the server and re-fetches.
 */
export const serverPlayersSchema = z.object({
  players: z.array(z.string()),
  online: z.number(),
  max: z.number().nullable(),
  supported: z.boolean(),
  sourceLine: z.string().nullable(),
  stale: z.boolean(),
  fetchedAt: z.coerce.date(),
});
