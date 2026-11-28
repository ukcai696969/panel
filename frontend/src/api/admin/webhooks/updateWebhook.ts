import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { webhookUpdateSchema } from '@/lib/schemas/admin/webhooks.ts';
import { formExtensionSchemas, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (
  uuid: string,
  webhookData: z.infer<typeof webhookUpdateSchema>,
): Promise<void> => {
  await axiosInstance.patch(
    `/api/admin/api-webhooks/${uuid}`,
    serializeForApi(
      webhookUpdateSchema,
      webhookData,
      formExtensionSchemas('admin.webhooks.createOrUpdate'),
    ),
  );
};
