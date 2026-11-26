import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverTemplateCreateSchema, serverTemplateSchema } from '@/lib/schemas/serverTemplates.ts';
import { parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  data: z.infer<typeof serverTemplateCreateSchema>,
): Promise<z.infer<typeof serverTemplateSchema>> => {
  const { data: response } = await axiosInstance.post(
    '/api/client/server-templates',
    serializeForApi(serverTemplateCreateSchema, data),
  );
  return parseFromApi(serverTemplateSchema, response.server_template);
};
