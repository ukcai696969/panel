import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { webhookDeliverySchema } from '@/lib/schemas/admin/webhooks.ts';
import { parsePaginationFromApi } from '@/lib/serialization/api-transform.ts';

export default async (
  uuid: string,
  page: number,
  perPage?: number,
): Promise<Pagination<z.infer<typeof webhookDeliverySchema>>> => {
  const { data } = await axiosInstance.get(`/api/admin/api-webhooks/${uuid}/deliveries`, {
    params: { page, per_page: perPage },
  });
  return parsePaginationFromApi(webhookDeliverySchema, data.deliveries);
};
