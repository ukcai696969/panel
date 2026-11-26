import { useEffect, useState } from 'react';
import { z } from 'zod';
import getEnvironment from '@/api/admin/settings/getEnvironment.ts';
import { httpErrorToHuman } from '@/api/axios.ts';
import Spinner from '@/elements/feedback/Spinner.tsx';
import { adminSettingsEnvironmentSchema } from '@/lib/schemas/admin/settings.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

type Environment = z.infer<typeof adminSettingsEnvironmentSchema>;

/**
 * Read-only status of the restart-only environment variables that have no
 * editable setting on this tab. Display-only: there is no edit path here —
 * these change in `.env` (or compose) plus a panel restart. Secrets are
 * never returned by the endpoint, not even masked: only "set / not set"
 * is shown for them.
 */
export default function EnvironmentSection() {
  const { addToast } = useToast();
  const { t } = useTranslations();
  const [environment, setEnvironment] = useState<Environment | null>(null);

  useEffect(() => {
    getEnvironment()
      .then(setEnvironment)
      .catch((msg) => addToast(httpErrorToHuman(msg), 'error'));
  }, []);

  return (
    <div style={{ marginTop: '2rem' }}>
      <h3>{t('pages.admin.settings.tabs.advanced.page.environment.title', {})}</h3>
      <p>{t('pages.admin.settings.tabs.advanced.page.environment.description', {})}</p>

      {!environment ? (
        <Spinner.Centered />
      ) : (
        <table style={{ width: '100%', borderCollapse: 'collapse' }}>
          <thead>
            <tr>
              <th style={{ textAlign: 'left', padding: '0.5rem 0.5rem 0.5rem 0' }}>
                {t('pages.admin.settings.tabs.advanced.page.environment.column.variable', {})}
              </th>
              <th style={{ textAlign: 'left', padding: '0.5rem' }}>
                {t('pages.admin.settings.tabs.advanced.page.environment.column.status', {})}
              </th>
              <th style={{ textAlign: 'left', padding: '0.5rem' }}>
                {t('pages.admin.settings.tabs.advanced.page.environment.column.value', {})}
              </th>
              <th style={{ textAlign: 'left', padding: '0.5rem' }}>
                {t('pages.admin.settings.tabs.advanced.page.environment.column.description', {})}
              </th>
            </tr>
          </thead>
          <tbody>
            {environment.entries.map((entry) => (
              <tr key={entry.name} style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}>
                <td style={{ padding: '0.5rem 0.5rem 0.5rem 0', fontFamily: 'monospace', whiteSpace: 'nowrap' }}>
                  {entry.name}
                </td>
                <td style={{ padding: '0.5rem', whiteSpace: 'nowrap' }}>
                  {entry.set
                    ? t('pages.admin.settings.tabs.advanced.page.environment.set', {})
                    : t('pages.admin.settings.tabs.advanced.page.environment.notSet', {})}
                </td>
                <td style={{ padding: '0.5rem', fontFamily: 'monospace' }}>
                  {entry.secret
                    ? '••••••'
                    : (entry.value ?? '—')}
                </td>
                <td style={{ padding: '0.5rem' }}>{entry.description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
