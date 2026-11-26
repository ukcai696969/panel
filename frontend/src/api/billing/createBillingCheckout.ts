import { axiosInstance } from '@/api/axios.ts';

export interface BillingCheckout {
  session_id: string;
  checkout_url: string;
  credits: number;
}

export default async (credits: number): Promise<BillingCheckout> => {
  const { data } = await axiosInstance.post('/api/client/billing/checkout', { credits });
  return data as BillingCheckout;
};
