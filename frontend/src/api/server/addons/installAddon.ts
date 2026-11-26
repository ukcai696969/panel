import { axiosInstance } from '@/api/axios.ts';
import type { AddonSource } from './searchAddons.ts';

export interface InstallAddonData {
  source?: AddonSource;
  projectId: string;
  versionId: string;
  projectType: string;
  fileName?: string | null;
  destination?: string | null;
  foreground?: boolean;
}

/**
 * Installs an add-on. Returns the Wings file-operation identifier when
 * the download runs in the background (202), or null when it finished
 * synchronously. Progress/completion for background pulls is reported by
 * the existing file-operation websocket listener (see
 * `WebsocketListener` operations.pulling toasts), the same as Files > Pull.
 */
export default async (uuid: string, installData: InstallAddonData): Promise<string | null> => {
  const { data } = await axiosInstance.post(`/api/client/servers/${uuid}/addons/install`, {
    source: installData.source ?? 'modrinth',
    project_id: installData.projectId,
    version_id: installData.versionId,
    project_type: installData.projectType,
    file_name: installData.fileName ?? null,
    destination: installData.destination ?? null,
    foreground: installData.foreground ?? false,
  });
  return (data.identifier as string | undefined) ?? null;
};
