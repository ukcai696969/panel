import { useEffect, useState } from 'react';
import { Route, Routes, useNavigate } from 'react-router';
import adjustUserBilling from '@/api/admin/billing/adjustUserBilling.ts';
import getBillingBalances, { type BillingBalanceRow } from '@/api/admin/billing/getBillingBalances.ts';
import getBillingPricing, { type BillingPricingSettings } from '@/api/admin/billing/getBillingPricing.ts';
import getBillingStripe, { type BillingStripeSettings } from '@/api/admin/billing/getBillingStripe.ts';
import getBillingTransactions, {
  type AdminBillingTransaction,
} from '@/api/admin/billing/getBillingTransactions.ts';
import updateBillingPricing from '@/api/admin/billing/updateBillingPricing.ts';
import updateBillingStripe, { type BillingStripePayload } from '@/api/admin/billing/updateBillingStripe.ts';
import Button from '@/elements/buttons/Button.tsx';
import AdminContentContainer from '@/elements/containers/AdminContentContainer.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

function formatCredits(credits: number) {
  return `${credits.toLocaleString()} credits`;
}

function BillingOverview() {
  const { t } = useTranslations();
  const navigate = useNavigate();
  const [pricing, setPricing] = useState<BillingPricingSettings | null>(null);
  const [balances, setBalances] = useState<BillingBalanceRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [adjustUser, setAdjustUser] = useState('');
  const [adjustAmount, setAdjustAmount] = useState('');
  const [adjustDescription, setAdjustDescription] = useState('');
  const [adjustResult, setAdjustResult] = useState<string | null>(null);
  const [stripe, setStripe] = useState<BillingStripeSettings | null>(null);
  const [stripeSecretKey, setStripeSecretKey] = useState('');
  const [stripeWebhookSecret, setStripeWebhookSecret] = useState('');
  const [stripePublishableKey, setStripePublishableKey] = useState('');
  const [stripeCheckoutTemplate, setStripeCheckoutTemplate] = useState('');
  const [stripeSaved, setStripeSaved] = useState(false);
  const [stripeError, setStripeError] = useState<string | null>(null);

  // Fill the Stripe form from a masked settings response. The secret
  // fields hold the masked value (e.g. `sk_test_…1234`); sending that
  // back unchanged is treated by the API as "leave unchanged", so the
  // form can safely send every field on save.
  const applyStripeSettings = (settings: BillingStripeSettings) => {
    setStripe(settings);
    setStripeSecretKey(settings.stripe_secret_key_masked ?? '');
    setStripeWebhookSecret(settings.stripe_webhook_secret_masked ?? '');
    setStripePublishableKey(settings.stripe_publishable_key ?? '');
    setStripeCheckoutTemplate(settings.checkout_url_template);
  };

  const saveStripe = () => {
    setStripeSaved(false);
    setStripeError(null);
    updateBillingStripe({
      stripe_secret_key: stripeSecretKey,
      stripe_webhook_secret: stripeWebhookSecret,
      stripe_publishable_key: stripePublishableKey,
      checkout_url_template: stripeCheckoutTemplate,
    })
      .then((settings) => {
        applyStripeSettings(settings);
        setStripeSaved(true);
      })
      .catch((err: unknown) => setStripeError(err instanceof Error ? err.message : String(err)));
  };

  // Sends an empty string for one key, which the API treats as "clear
  // the stored key".
  const clearStripeKey = (which: 'secret' | 'webhook') => {
    const payload: BillingStripePayload =
      which === 'secret' ? { stripe_secret_key: '' } : { stripe_webhook_secret: '' };
    setStripeSaved(false);
    setStripeError(null);
    updateBillingStripe(payload)
      .then((settings) => {
        applyStripeSettings(settings);
        setStripeSaved(true);
      })
      .catch((err: unknown) => setStripeError(err instanceof Error ? err.message : String(err)));
  };

  useEffect(() => {
    getBillingPricing()
      .then(setPricing)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
    getBillingBalances()
      .then((page) => setBalances(page.data))
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
    getBillingStripe()
      .then(applyStripeSettings)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  const savePricing = () => {
    if (!pricing) return;
    setSaved(false);
    setError(null);
    updateBillingPricing(pricing)
      .then(() => setSaved(true))
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  };

  const adjust = () => {
    const amount = Number.parseInt(adjustAmount, 10);
    if (!adjustUser || !Number.isInteger(amount) || amount === 0) {
      setAdjustResult('Enter a user UUID and a non-zero amount.');
      return;
    }
    adjustUserBilling(adjustUser, amount, adjustDescription || undefined)
      .then((balance) => {
        setAdjustResult(`New balance for ${balance.user_uuid}: ${formatCredits(balance.balance_credits)}`);
        return getBillingBalances();
      })
      .then((page) => setBalances(page.data))
      .catch((err: unknown) => setAdjustResult(err instanceof Error ? err.message : String(err)));
  };

  return (
    <div className='flex flex-col gap-6'>
      {error && <p className='text-red-500'>{error}</p>}
      {saved && <p className='text-green-500'>Pricing saved.</p>}

      <section>
        <h2 className='text-lg font-semibold mb-2'>Pricing</h2>
        {!pricing ? (
          <p>Loading…</p>
        ) : (
          <div className='flex flex-col gap-3 max-w-md'>
            <label className='flex items-center justify-between gap-4'>
              <span>Billing enabled</span>
              <input
                type='checkbox'
                checked={pricing.enabled}
                onChange={(e) => setPricing({ ...pricing, enabled: e.target.checked })}
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Currency (ISO code)</span>
              <input
                className='border rounded px-2 py-1 w-24'
                value={pricing.currency}
                maxLength={3}
                onChange={(e) => setPricing({ ...pricing, currency: e.target.value })}
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Credits / GiB RAM / hour</span>
              <input
                type='number'
                min={0}
                className='border rounded px-2 py-1 w-24'
                value={pricing.price_per_gb_ram_per_hour_credits}
                onChange={(e) =>
                  setPricing({ ...pricing, price_per_gb_ram_per_hour_credits: Number(e.target.value) })
                }
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Credits / 100 CPU / hour</span>
              <input
                type='number'
                min={0}
                className='border rounded px-2 py-1 w-24'
                value={pricing.price_per_100_cpu_per_hour_credits}
                onChange={(e) =>
                  setPricing({ ...pricing, price_per_100_cpu_per_hour_credits: Number(e.target.value) })
                }
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Credits / GiB disk / hour</span>
              <input
                type='number'
                min={0}
                className='border rounded px-2 py-1 w-24'
                value={pricing.price_per_gb_disk_per_hour_credits}
                onChange={(e) =>
                  setPricing({ ...pricing, price_per_gb_disk_per_hour_credits: Number(e.target.value) })
                }
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Suspend when balance goes below zero</span>
              <input
                type='checkbox'
                checked={pricing.suspend_on_negative_balance}
                onChange={(e) => setPricing({ ...pricing, suspend_on_negative_balance: e.target.checked })}
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Minimum purchase (credits)</span>
              <input
                type='number'
                min={1}
                className='border rounded px-2 py-1 w-24'
                value={pricing.minimum_purchase_credits}
                onChange={(e) => setPricing({ ...pricing, minimum_purchase_credits: Number(e.target.value) })}
              />
            </label>
            <label className='flex items-center justify-between gap-4'>
              <span>Maximum purchase (credits)</span>
              <input
                type='number'
                min={1}
                className='border rounded px-2 py-1 w-24'
                value={pricing.maximum_purchase_credits}
                onChange={(e) => setPricing({ ...pricing, maximum_purchase_credits: Number(e.target.value) })}
              />
            </label>
            <p className='text-sm opacity-70'>
              1 credit = 1 minor currency unit (1 cent for {pricing.currency || 'USD'}). Stripe
              keys are managed in the Stripe section below.
            </p>
            <div>
              <Button onClick={savePricing} color='blue'>
                Save pricing
              </Button>
            </div>
          </div>
        )}
      </section>

      <section>
        <h2 className='text-lg font-semibold mb-2'>Stripe</h2>
        <p className='text-sm opacity-70 max-w-md mb-3'>
          Test mode only. Stripe is still a stub: no live Stripe API call is made and no real
          charge happens. Live keys (sk_live_… / pk_live_…) are refused. The secret key and
          webhook secret are stored encrypted; the publishable key and checkout URL template
          are not secret. Environment variables (STRIPE_SECRET_KEY, STRIPE_WEBHOOK_SECRET,
          STRIPE_PUBLISHABLE_KEY, BILLING_CHECKOUT_URL_TEMPLATE) override the values saved
          here when set.
        </p>
        {!stripe ? (
          <p>Loading…</p>
        ) : (
          <div className='flex flex-col gap-3 max-w-md'>
            {stripeError && <p className='text-red-500'>{stripeError}</p>}
            {stripeSaved && <p className='text-green-500'>Stripe settings saved.</p>}
            <label className='flex items-center justify-between gap-4'>
              <span>Secret key</span>
              <input
                type='password'
                autoComplete='off'
                className='border rounded px-2 py-1 w-64 font-mono text-sm'
                placeholder='sk_test_…'
                value={stripeSecretKey}
                onChange={(e) => setStripeSecretKey(e.target.value)}
              />
            </label>
            <p className='text-sm opacity-70'>
              {stripe.stripe_secret_key_configured ? 'Configured' : 'Not configured'}
              {stripe.stripe_secret_key_from_env &&
                ' — set by the STRIPE_SECRET_KEY environment variable, which takes precedence over the saved key.'}{' '}
              Leave the masked value untouched to keep the saved key; clear the field and save
              to remove it.
            </p>
            <div>
              <Button onClick={() => clearStripeKey('secret')} color='red'>
                Clear secret key
              </Button>
            </div>
            <label className='flex items-center justify-between gap-4'>
              <span>Webhook secret</span>
              <input
                type='password'
                autoComplete='off'
                className='border rounded px-2 py-1 w-64 font-mono text-sm'
                placeholder='whsec_…'
                value={stripeWebhookSecret}
                onChange={(e) => setStripeWebhookSecret(e.target.value)}
              />
            </label>
            <p className='text-sm opacity-70'>
              {stripe.stripe_webhook_secret_configured ? 'Configured' : 'Not configured'}
              {stripe.stripe_webhook_secret_from_env &&
                ' — set by the STRIPE_WEBHOOK_SECRET environment variable, which takes precedence over the saved secret.'}{' '}
              Required for the webhook to accept events. Leave the masked value untouched to
              keep the saved secret; clear the field and save to remove it.
            </p>
            <div>
              <Button onClick={() => clearStripeKey('webhook')} color='red'>
                Clear webhook secret
              </Button>
            </div>
            <label className='flex items-center justify-between gap-4'>
              <span>Publishable key</span>
              <input
                className='border rounded px-2 py-1 w-64 font-mono text-sm'
                placeholder='pk_test_…'
                value={stripePublishableKey}
                onChange={(e) => setStripePublishableKey(e.target.value)}
              />
            </label>
            <p className='text-sm opacity-70'>
              Public by design — sent to the browser.
              {stripe.stripe_publishable_key_from_env &&
                ' Set by the STRIPE_PUBLISHABLE_KEY environment variable, which takes precedence over the saved key.'}
            </p>
            <label className='flex items-center justify-between gap-4'>
              <span>Checkout URL template</span>
              <input
                className='border rounded px-2 py-1 w-64 font-mono text-sm'
                placeholder='{app_url}/account/billing?checkout_session={session_id}'
                value={stripeCheckoutTemplate}
                onChange={(e) => setStripeCheckoutTemplate(e.target.value)}
              />
            </label>
            <p className='text-sm opacity-70'>
              Must contain {'{session_id}'}; also supports {'{credits}'} and {'{app_url}'}.
              Empty resets to the default.
              {stripe.checkout_url_template_from_env &&
                ' Set by the BILLING_CHECKOUT_URL_TEMPLATE environment variable, which takes precedence over the saved template.'}
            </p>
            <div>
              <Button onClick={saveStripe} color='blue'>
                Save Stripe settings
              </Button>
            </div>
          </div>
        )}
      </section>

      <section>
        <h2 className='text-lg font-semibold mb-2'>Grant / adjust credits</h2>
        <div className='flex flex-col gap-3 max-w-md'>
          <input
            className='border rounded px-2 py-1'
            placeholder='User UUID'
            value={adjustUser}
            onChange={(e) => setAdjustUser(e.target.value)}
          />
          <input
            type='number'
            className='border rounded px-2 py-1'
            placeholder='Amount in credits (negative removes)'
            value={adjustAmount}
            onChange={(e) => setAdjustAmount(e.target.value)}
          />
          <input
            className='border rounded px-2 py-1'
            placeholder='Reason (shown in the ledger)'
            value={adjustDescription}
            onChange={(e) => setAdjustDescription(e.target.value)}
          />
          <div>
            <Button onClick={adjust} color='blue'>
              Apply adjustment
            </Button>
          </div>
          {adjustResult && <p className='text-sm'>{adjustResult}</p>}
        </div>
      </section>

      <section>
        <div className='flex items-center gap-4 mb-2'>
          <h2 className='text-lg font-semibold'>Balances</h2>
          <Button onClick={() => navigate('/admin/billing/transactions')} color='blue'>
            View all transactions
          </Button>
        </div>
        {balances.length === 0 ? (
          <p className='text-sm opacity-70'>No balances yet.</p>
        ) : (
          <table className='w-full text-left text-sm'>
            <thead>
              <tr>
                <th className='py-1 pr-2'>User UUID</th>
                <th className='py-1 pr-2'>Balance</th>
                <th className='py-1'>Updated</th>
              </tr>
            </thead>
            <tbody>
              {balances.map((row) => (
                <tr key={row.uuid}>
                  <td className='py-1 pr-2 font-mono text-xs'>{row.user_uuid}</td>
                  <td className='py-1 pr-2'>{formatCredits(row.balance_credits)}</td>
                  <td className='py-1'>{new Date(row.updated).toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}

function BillingTransactions() {
  const [transactions, setTransactions] = useState<AdminBillingTransaction[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getBillingTransactions()
      .then((page) => setTransactions(page.data))
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  return (
    <div className='flex flex-col gap-4'>
      {error && <p className='text-red-500'>{error}</p>}
      {transactions.length === 0 ? (
        <p className='text-sm opacity-70'>No transactions yet.</p>
      ) : (
        <table className='w-full text-left text-sm'>
          <thead>
            <tr>
              <th className='py-1 pr-2'>When</th>
              <th className='py-1 pr-2'>User</th>
              <th className='py-1 pr-2'>Kind</th>
              <th className='py-1 pr-2'>Amount</th>
              <th className='py-1 pr-2'>Balance after</th>
              <th className='py-1'>Reference</th>
            </tr>
          </thead>
          <tbody>
            {transactions.map((tx) => (
              <tr key={tx.uuid}>
                <td className='py-1 pr-2'>{new Date(tx.created).toLocaleString()}</td>
                <td className='py-1 pr-2 font-mono text-xs'>{tx.user_uuid}</td>
                <td className='py-1 pr-2'>{tx.kind}</td>
                <td className='py-1 pr-2'>{formatCredits(tx.amount_credits)}</td>
                <td className='py-1 pr-2'>{formatCredits(tx.balance_after_credits)}</td>
                <td className='py-1 font-mono text-xs'>{tx.reference ?? '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export default function AdminBilling() {
  const { t } = useTranslations();

  return (
    <AdminContentContainer title={t('pages.admin.billing.title', {})}>
      <Routes>
        <Route path='/' element={<BillingOverview />} />
        <Route path='/transactions' element={<BillingTransactions />} />
      </Routes>
    </AdminContentContainer>
  );
}
