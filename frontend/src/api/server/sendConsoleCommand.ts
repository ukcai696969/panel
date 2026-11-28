import { axiosInstance } from '@/api/axios.ts';

/**
 * Sends one command to the server console through the existing client API
 * (`POST /api/client/servers/{server}/command`), which forwards it to Wings
 * (`POST /api/server/{server}/commands`) and records it in the server
 * activity log. Requires the `control.console` permission, enforced by the
 * backend. Used by the player actions (kick/ban/whitelist) and the
 * quick-command bar, so those never need to touch the websocket directly.
 */
export default async (uuid: string, command: string): Promise<void> => {
  await axiosInstance.post(`/api/client/servers/${uuid}/command`, {
    command,
  });
};
