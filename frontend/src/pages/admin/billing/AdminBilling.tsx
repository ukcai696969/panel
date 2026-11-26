import { useEffect, useState } from 'react';
import { Route, Routes, useNavigate } from 'react-router';
import adjustUserBilling from '@/api/admin/billing/adjustUserBilling.ts';
import getBillingBalances, { type BillingBalanceRow } from '@/api/admin/billing/getBillingBalances.ts';
import getBillingPricing, { type BillingPricingSettings } from '@/api/admin/billing/getBillingPricing.ts';
import getBillingTransactions, {
  type AdminBillingTransaction,
} from '@/api/admin/billing/getBillingTransactions.ts';
import updateBillingPricing from '@/api/admin/billing/updateBillingPricing.ts';
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

  useEffect(() => {
    getBillingPricing()
      .then(setPricing)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
    getBillingBalances()
      .then((page) => setBalances(page.data))
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
              secrets are environment variables only and are never stored here.
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
