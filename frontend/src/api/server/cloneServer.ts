import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverSchema } from '@/lib/schemas/server/server.ts';
import { serverTemplateCloneSchema } from '@/lib/schemas/serverTemplates.ts';
import { parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  serverUuid: string,
  data: z.infer<typeof serverTemplateCloneSchema>,
): Promise<z.infer<typeof serverSchema>> => {
  const { data: response } = await axiosInstance.post(
    `/api/client/servers/${serverUuid}/clone`,
    serializeForApi(serverTemplateCloneSchema, data),
  );
  return parseFromApi(serverSchema, response.server);
};
