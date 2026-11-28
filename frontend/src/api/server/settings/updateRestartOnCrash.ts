import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverSettingsRestartOnCrashSchema } from '@/lib/schemas/server/settings.ts';
import { serializeForApi } from '@/lib/serialization/api-transform.ts';

export default async (uuid: string, data: z.infer<typeof serverSettingsRestartOnCrashSchema>): Promise<void> => {
  await axiosInstance.put(
    `/api/client/servers/${uuid}/settings/restart-on-crash`,
    serializeForApi(serverSettingsRestartOnCrashSchema, data),
  );
};
