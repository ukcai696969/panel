import { axiosInstance } from '@/api/axios.ts';

export interface BillingBalanceRow {
  uuid: string;
  user_uuid: string;
  balance_credits: number;
  created: string;
  updated: string;
}

export interface BillingBalancesPage {
  total: number;
  per_page: number;
  page: number;
  data: BillingBalanceRow[];
}

export default async (page = 1, search?: string, perPage = 25): Promise<BillingBalancesPage> => {
  const { data } = await axiosInstance.get('/api/admin/billing/balances', {
    params: { page, per_page: perPage, search },
  });
  return data.balances as BillingBalancesPage;
};
