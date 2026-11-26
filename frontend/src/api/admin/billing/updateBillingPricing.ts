import { axiosInstance } from '@/api/axios.ts';
import type { BillingPricingSettings } from './getBillingPricing.ts';

export default async (pricing: Partial<BillingPricingSettings>): Promise<void> => {
  await axiosInstance.put('/api/admin/billing/pricing', pricing);
};
