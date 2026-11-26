import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminSettingsAdvancedSchema } from '@/lib/schemas/admin/settings.ts';
import { formExtensionSchemas, serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (data: z.infer<typeof adminSettingsAdvancedSchema>): Promise<void> => {
  await axiosInstance.put('/api/admin/settings', {
    advanced: serializeForApi(adminSettingsAdvancedSchema, data, formExtensionSchemas('admin.settings.advanced')),
  });
};
