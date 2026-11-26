import { axiosInstance } from '@/api/axios.ts';

export interface BillingPricingSettings {
  enabled: boolean;
  currency: string;
  price_per_gb_ram_per_hour_credits: number;
  price_per_100_cpu_per_hour_credits: number;
  price_per_gb_disk_per_hour_credits: number;
  suspend_on_negative_balance: boolean;
  minimum_purchase_credits: number;
  maximum_purchase_credits: number;
}

export default async (): Promise<BillingPricingSettings> => {
  const { data } = await axiosInstance.get('/api/admin/billing/pricing');
  return data.billing as BillingPricingSettings;
};
