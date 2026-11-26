import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminServerSchema } from '@/lib/schemas/admin/servers.ts';
import { serverTemplateCloneSchema } from '@/lib/schemas/serverTemplates.ts';
import { parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  serverUuid: string,
  data: z.infer<typeof serverTemplateCloneSchema>,
): Promise<z.infer<typeof adminServerSchema>> => {
  const { data: response } = await axiosInstance.post(
    `/api/admin/servers/${serverUuid}/clone`,
    serializeForApi(serverTemplateCloneSchema, data),
  );
  return parseFromApi(adminServerSchema, response.server);
};
