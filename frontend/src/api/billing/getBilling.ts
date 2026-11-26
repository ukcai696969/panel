import { axiosInstance } from '@/api/axios.ts';

export interface BillingPricing {
  enabled: boolean;
  currency: string;
  price_per_gb_ram_per_hour_credits: number;
  price_per_100_cpu_per_hour_credits: number;
  price_per_gb_disk_per_hour_credits: number;
  minimum_purchase_credits: number;
  maximum_purchase_credits: number;
  stripe_publishable_key: string | null;
  stripe_configured: boolean;
}

export interface BillingBalance {
  balance_credits: number;
  updated: string | null;
}

export interface ClientBilling {
  balance: BillingBalance;
  pricing: BillingPricing;
}

export default async (): Promise<ClientBilling> => {
  const { data } = await axiosInstance.get('/api/client/billing');
  return data as ClientBilling;
};
