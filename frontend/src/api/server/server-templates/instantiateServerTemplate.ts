import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverSchema } from '@/lib/schemas/server/server.ts';
import { serverTemplateInstantiateSchema } from '@/lib/schemas/serverTemplates.ts';
import { parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  templateUuid: string,
  data: z.infer<typeof serverTemplateInstantiateSchema>,
): Promise<z.infer<typeof serverSchema>> => {
  const { data: response } = await axiosInstance.post(
    `/api/client/server-templates/${templateUuid}/instantiate`,
    serializeForApi(serverTemplateInstantiateSchema, data),
  );
  return parseFromApi(serverSchema, response.server);
};
