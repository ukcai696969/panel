import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminSettingsEnvironmentSchema } from '@/lib/schemas/admin/settings.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';

export default async (): Promise<z.infer<typeof adminSettingsEnvironmentSchema>> => {
  const { data } = await axiosInstance.get('/api/admin/settings/environment');
  return parseFromApi(adminSettingsEnvironmentSchema, data);
};
