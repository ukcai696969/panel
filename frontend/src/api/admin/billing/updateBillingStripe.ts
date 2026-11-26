import { axiosInstance } from '@/api/axios.ts';
import type { BillingStripeSettings } from './getBillingStripe.ts';

export interface BillingStripePayload {
  stripe_secret_key?: string;
  stripe_webhook_secret?: string;
  stripe_publishable_key?: string;
  checkout_url_template?: string;
}

export default async (stripe: BillingStripePayload): Promise<BillingStripeSettings> => {
  const { data } = await axiosInstance.put('/api/admin/billing/stripe', stripe);
  return data as BillingStripeSettings;
};
