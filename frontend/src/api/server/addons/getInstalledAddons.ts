import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverActivitySchema } from '@/lib/schemas/server/activity.ts';
import { parsePaginationFromApi } from '@/lib/serialization/api-transform.ts';

/**
 * Installed add-ons, from the `server:addon.install` activity entries
 * recorded by the install endpoint. Each entry's `data` holds
 * `{ source, project_id, project_type, version_id, version_number, file,
 * destination }`.
 */
export default async (
  uuid: string,
  page: number = 1,
  perPage: number = 10,
): Promise<Pagination<z.infer<typeof serverActivitySchema>>> => {
  const { data } = await axiosInstance.get(`/api/client/servers/${uuid}/addons/installed`, {
    params: { page, per_page: perPage },
  });
  return parsePaginationFromApi(serverActivitySchema, data.installs);
};
