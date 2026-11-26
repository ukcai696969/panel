import { axiosInstance } from '@/api/axios.ts';

export default async (uuid: string, webhookUuid: string): Promise<void> => {
  await axiosInstance.delete(`/api/client/servers/${uuid}/webhooks/${webhookUuid}`);
};
