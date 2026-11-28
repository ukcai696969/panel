import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { webhookTestResultSchema } from '@/lib/schemas/admin/webhooks.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';

export default async (uuid: string): Promise<z.infer<typeof webhookTestResultSchema>> => {
  const { data } = await axiosInstance.post(`/api/admin/api-webhooks/${uuid}/test`);
  return parseFromApi(webhookTestResultSchema, data);
};
