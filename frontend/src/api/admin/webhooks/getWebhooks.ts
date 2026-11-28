import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { webhookSchema } from '@/lib/schemas/admin/webhooks.ts';
import { parsePaginationFromApi } from '@/lib/serialization/api-transform.ts';

export default async (
  page: number,
  search?: string,
  perPage?: number,
): Promise<Pagination<z.infer<typeof webhookSchema>>> => {
  const { data } = await axiosInstance.get('/api/admin/api-webhooks', {
    params: { page, per_page: perPage, search },
  });
  return parsePaginationFromApi(webhookSchema, data.webhooks);
};
