import { axiosInstance } from '@/api/axios.ts';

export interface BillingStripeSettings {
  stripe_secret_key_masked: string | null;
  stripe_secret_key_configured: boolean;
  stripe_secret_key_from_env: boolean;
  stripe_webhook_secret_masked: string | null;
  stripe_webhook_secret_configured: boolean;
  stripe_webhook_secret_from_env: boolean;
  stripe_publishable_key: string | null;
  stripe_publishable_key_from_env: boolean;
  checkout_url_template: string;
  checkout_url_template_from_env: boolean;
  stripe_configured: boolean;
}

export default async (): Promise<BillingStripeSettings> => {
  const { data } = await axiosInstance.get('/api/admin/billing/stripe');
  return data as BillingStripeSettings;
};
