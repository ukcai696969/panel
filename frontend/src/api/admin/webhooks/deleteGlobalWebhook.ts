import { axiosInstance } from '@/api/axios.ts';

export default async (webhookUuid: string): Promise<void> => {
  await axiosInstance.delete(`/api/admin/webhooks/${webhookUuid}`);
};
