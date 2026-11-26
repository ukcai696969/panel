import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminServerAlertSchema } from '@/lib/schemas/server/webhooks.ts';
import { parsePaginationFromApi } from '@/lib/serialization/api-transform.ts';

export default async (
  page: number,
  search?: string,
  perPage?: number,
): Promise<Pagination<z.infer<typeof adminServerAlertSchema>>> => {
  const { data } = await axiosInstance.get('/api/admin/alerts', {
    params: { page, search, per_page: perPage },
  });
  return parsePaginationFromApi(adminServerAlertSchema, data.alerts);
};
