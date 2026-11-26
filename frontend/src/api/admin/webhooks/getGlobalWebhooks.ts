import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverWebhookSchema } from '@/lib/schemas/server/webhooks.ts';
import { parsePaginationFromApi } from '@/lib/serialization/api-transform.ts';

export default async (
  page: number,
  search?: string,
  perPage?: number,
): Promise<Pagination<z.infer<typeof serverWebhookSchema>>> => {
  const { data } = await axiosInstance.get('/api/admin/webhooks', {
    params: { page, search, per_page: perPage },
  });
  return parsePaginationFromApi(serverWebhookSchema, data.webhooks);
};
