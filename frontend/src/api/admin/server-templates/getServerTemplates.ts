import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverTemplateSchema } from '@/lib/schemas/serverTemplates.ts';
import { parsePaginationFromApi } from '@/lib/serialization/api-transform.ts';

export default async (
  page: number,
  search?: string,
  perPage?: number,
): Promise<Pagination<z.infer<typeof serverTemplateSchema>>> => {
  const { data } = await axiosInstance.get('/api/admin/server-templates', {
    params: { page, per_page: perPage, search },
  });
  return parsePaginationFromApi(serverTemplateSchema, data.server_templates);
};
