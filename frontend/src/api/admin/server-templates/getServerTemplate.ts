import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminServerTemplateSchema } from '@/lib/schemas/serverTemplates.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';

export default async (templateUuid: string): Promise<z.infer<typeof adminServerTemplateSchema>> => {
  const { data } = await axiosInstance.get(`/api/admin/server-templates/${templateUuid}`);
  return parseFromApi(adminServerTemplateSchema, data.server_template);
};
