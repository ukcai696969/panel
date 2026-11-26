import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import {
  serverWebhookCreateResponseSchema,
  serverWebhookCreateSchema,
} from '@/lib/schemas/server/webhooks.ts';
import { parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  webhookData: z.infer<typeof serverWebhookCreateSchema>,
): Promise<z.infer<typeof serverWebhookCreateResponseSchema>> => {
  const { data } = await axiosInstance.post(
    '/api/admin/webhooks',
    serializeForApi(serverWebhookCreateSchema, webhookData),
  );
  return parseFromApi(serverWebhookCreateResponseSchema, data);
};
