import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { addonVersionsResultSchema } from '@/lib/schemas/server/addons.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';
import type { AddonSource } from './searchAddons.ts';

export default async (
  uuid: string,
  projectId: string,
  source: AddonSource = 'modrinth',
): Promise<z.infer<typeof addonVersionsResultSchema>> => {
  const { data } = await axiosInstance.get(`/api/client/servers/${uuid}/addons/versions`, {
    params: { project_id: projectId, source },
  });
  return parseFromApi(addonVersionsResultSchema, data);
};
