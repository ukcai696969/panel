import { axiosInstance } from '@/api/axios.ts';

export interface AdminBillingTransaction {
  uuid: string;
  user_uuid: string;
  kind: 'purchase' | 'charge' | 'refund' | 'adjustment';
  amount_credits: number;
  balance_after_credits: number;
  server_uuid: string | null;
  reference: string | null;
  description: string | null;
  created: string;
}

export interface AdminBillingTransactionsPage {
  total: number;
  per_page: number;
  page: number;
  data: AdminBillingTransaction[];
}

export default async (page = 1, search?: string, perPage = 25): Promise<AdminBillingTransactionsPage> => {
  const { data } = await axiosInstance.get('/api/admin/billing/transactions', {
    params: { page, per_page: perPage, search },
  });
  return data.transactions as AdminBillingTransactionsPage;
};
