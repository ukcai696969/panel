import { axiosInstance } from '@/api/axios.ts';

export interface BillingTransaction {
  uuid: string;
  kind: 'purchase' | 'charge' | 'refund' | 'adjustment';
  amount_credits: number;
  balance_after_credits: number;
  server_uuid: string | null;
  description: string | null;
  created: string;
}

export interface BillingTransactionsPage {
  total: number;
  per_page: number;
  page: number;
  data: BillingTransaction[];
}

export default async (page = 1, perPage = 25): Promise<BillingTransactionsPage> => {
  const { data } = await axiosInstance.get('/api/client/billing/transactions', {
    params: { page, per_page: perPage },
  });
  return data.transactions as BillingTransactionsPage;
};
