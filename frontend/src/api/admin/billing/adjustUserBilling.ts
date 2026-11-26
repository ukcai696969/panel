import { axiosInstance } from '@/api/axios.ts';

export interface AdjustedBalance {
  uuid: string;
  user_uuid: string;
  balance_credits: number;
  created: string;
  updated: string;
}

export default async (
  userUuid: string,
  amountCredits: number,
  description?: string,
): Promise<AdjustedBalance> => {
  const { data } = await axiosInstance.post(`/api/admin/billing/users/${userUuid}/adjust`, {
    amount_credits: amountCredits,
    description,
  });
  return data.balance as AdjustedBalance;
};
