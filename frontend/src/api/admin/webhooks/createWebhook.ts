import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { webhookCreateSchema, webhookSchema } from '@/lib/schemas/admin/webhooks.ts';
import { formExtensionSchemas, parseFromApi, serializeForApi } from '@/lib/serialization/api-transform.ts';

export interface CreatedWebhook {
  webhook: z.infer<typeof webhookSchema>;
  /** Plaintext signing secret, returned exactly once. */
  secret: string;
}

export default async (
  webhookData: z.infer<typeof webhookCreateSchema>,
): Promise<CreatedWebhook> => {
  const { data } = await axiosInstance.post(
    '/api/admin/api-webhooks',
    serializeForApi(
      webhookCreateSchema,
      webhookData,
      formExtensionSchemas('admin.webhooks.createOrUpdate'),
    ),
  );
  return {
    webhook: parseFromApi(webhookSchema, data.webhook),
    secret: data.secret as string,
  };
};
