import { axiosInstance } from '@/api/axios.ts';

/** Returns the new plaintext signing secret, shown exactly once. */
export default async (uuid: string): Promise<string> => {
  const { data } = await axiosInstance.post(`/api/admin/api-webhooks/${uuid}/rotate-secret`);
  return data.secret as string;
};
