import { useEffect, useState } from 'react';
import createBillingCheckout from '@/api/billing/createBillingCheckout.ts';
import getBilling, { type ClientBilling } from '@/api/billing/getBilling.ts';
import getBillingTransactions, {
  type BillingTransaction,
} from '@/api/billing/getBillingTransactions.ts';
import Button from '@/elements/buttons/Button.tsx';
import AccountContentContainer from '@/elements/containers/AccountContentContainer.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

export default function DashboardBilling() {
  const { t } = useTranslations();
  const [billing, setBilling] = useState<ClientBilling | null>(null);
  const [transactions, setTransactions] = useState<BillingTransaction[]>([]);
  const [credits, setCredits] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    getBilling()
      .then(setBilling)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
    getBillingTransactions()
      .then((page) => setTransactions(page.data))
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  const buy = () => {
    const amount = Number.parseInt(credits, 10);
    if (!billing || !Number.isInteger(amount) || amount <= 0) {
      setNotice('Enter how many credits to buy.');
      return;
    }
    setNotice(null);
    setError(null);
    createBillingCheckout(amount)
      .then((session) => {
        // The checkout URL is a Stripe Checkout URL in the real
        // integration. In this first version it is the stub URL from
        // the server (see backend/src/billing.rs) — no charge is made.
        window.location.href = session.checkout_url;
      })
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  };

  const pricing = billing?.pricing;

  return (
    <AccountContentContainer title={t('pages.account.billing.title', {})}>
      <div className='flex flex-col gap-6'>
        {error && <p className='text-red-500'>{error}</p>}
        {notice && <p className='text-sm'>{notice}</p>}

        {!billing ? (
          <p>Loading…</p>
        ) : !pricing?.enabled ? (
          <p>Billing is not enabled on this panel, so servers are not charged.</p>
        ) : (
          <>
            <section>
              <h2 className='text-lg font-semibold mb-1'>Balance</h2>
              <p className='text-2xl'>{billing.balance.balance_credits.toLocaleString()} credits</p>
              <p className='text-sm opacity-70'>
                1 credit = 1 cent ({pricing.currency}). Servers cost{' '}
                {pricing.price_per_gb_ram_per_hour_credits} credits / GiB RAM / hour,{' '}
                {pricing.price_per_100_cpu_per_hour_credits} credits / 100 CPU / hour and{' '}
                {pricing.price_per_gb_disk_per_hour_credits} credits / GiB disk / hour, charged
                hourly while a server is running.
              </p>
              {!pricing.stripe_configured && (
                <p className='text-sm text-yellow-500'>
                  Card payments are not configured yet — ask an admin to set up Stripe.
                </p>
              )}
            </section>

            <section>
              <h2 className='text-lg font-semibold mb-2'>Buy credits</h2>
              <div className='flex gap-2 max-w-md'>
                <input
                  type='number'
                  min={pricing.minimum_purchase_credits}
                  max={pricing.maximum_purchase_credits}
                  className='border rounded px-2 py-1 flex-1'
                  placeholder={`Credits (${pricing.minimum_purchase_credits}–${pricing.maximum_purchase_credits})`}
                  value={credits}
                  onChange={(e) => setCredits(e.target.value)}
                />
                <Button onClick={buy} color='blue'>
                  Buy
                </Button>
              </div>
              <p className='text-sm opacity-70 mt-1'>
                You will be taken to Stripe Checkout. Credits are added when the payment
                completes.
              </p>
            </section>
          </>
        )}

        <section>
          <h2 className='text-lg font-semibold mb-2'>History</h2>
          {transactions.length === 0 ? (
            <p className='text-sm opacity-70'>No transactions yet.</p>
          ) : (
            <table className='w-full text-left text-sm'>
              <thead>
                <tr>
                  <th className='py-1 pr-2'>When</th>
                  <th className='py-1 pr-2'>Kind</th>
                  <th className='py-1 pr-2'>Amount</th>
                  <th className='py-1 pr-2'>Balance after</th>
                  <th className='py-1'>Details</th>
                </tr>
              </thead>
              <tbody>
                {transactions.map((tx) => (
                  <tr key={tx.uuid}>
                    <td className='py-1 pr-2'>{new Date(tx.created).toLocaleString()}</td>
                    <td className='py-1 pr-2'>{tx.kind}</td>
                    <td className='py-1 pr-2'>{tx.amount_credits.toLocaleString()} credits</td>
                    <td className='py-1 pr-2'>{tx.balance_after_credits.toLocaleString()} credits</td>
                    <td className='py-1'>{tx.description ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      </div>
    </AccountContentContainer>
  );
}
