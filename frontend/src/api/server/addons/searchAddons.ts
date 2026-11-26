import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { addonSearchResultSchema } from '@/lib/schemas/server/addons.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';

export type AddonSource = 'modrinth' | 'curseforge';
export type AddonProjectType = 'mod' | 'modpack' | 'plugin' | 'datapack' | 'shader' | 'resourcepack';

export default async (
  uuid: string,
  query: string,
  options?: { projectType?: AddonProjectType | ''; source?: AddonSource; page?: number; perPage?: number },
): Promise<z.infer<typeof addonSearchResultSchema>> => {
  const { data } = await axiosInstance.get(`/api/client/servers/${uuid}/addons/search`, {
    params: {
      query,
      project_type: options?.projectType || undefined,
      source: options?.source ?? 'modrinth',
      page: options?.page ?? 1,
      per_page: options?.perPage ?? 20,
    },
  });
  return parseFromApi(addonSearchResultSchema, data);
};
