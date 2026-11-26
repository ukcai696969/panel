import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminServerSchema } from '@/lib/schemas/admin/servers.ts';
import { serverTemplateInstantiateSchema } from '@/lib/schemas/serverTemplates.ts';
import { parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  templateUuid: string,
  data: z.infer<typeof serverTemplateInstantiateSchema>,
): Promise<z.infer<typeof adminServerSchema>> => {
  const { data: response } = await axiosInstance.post(
    `/api/admin/server-templates/${templateUuid}/instantiate`,
    serializeForApi(serverTemplateInstantiateSchema, data),
  );
  return parseFromApi(adminServerSchema, response.server);
};
