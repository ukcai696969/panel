import { faBell } from '@fortawesome/free-solid-svg-icons';
import getAdminAlerts from '@/api/admin/alerts/getAdminAlerts.ts';
import Badge from '@/elements/data-display/Badge.tsx';
import EmptyState from '@/elements/feedback/EmptyState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Paper from '@/elements/layout/Paper.tsx';
import FormattedTimestamp from '@/elements/time/FormattedTimestamp.tsx';
import Text from '@/elements/typography/Text.tsx';
import { queryKeys } from '@/lib/queryKeys.ts';
import { useSearchablePaginatedTable } from '@/plugins/resource/useSearchablePaginatedTable.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

// Admin Notifications log: every crash/offline/recovery alert the
// watcher raised, across all servers, newest first.
//
// WIRING NOTE (see MANIFEST.md): register this page in the admin
// router (frontend/src/routers/routes/adminRoutes.ts or equivalent)
// under `/admin/notifications`, inside the admin layout, and add the
// `admin.alerts.all()` query key to frontend/src/lib/queryKeys.ts.
// It is staged here as a standalone page so the API wiring can be
// reviewed without guessing at the admin layout's container API.
export default function AdminNotifications() {
  const { t } = useTranslations();

  const {
    data: alerts,
    loading,
    error,
    search,
    setSearch,
    setPage,
    setPerPage,
  } = useSearchablePaginatedTable({
    queryKey: queryKeys.admin.alerts.all(),
    fetcher: (page, search, perPage) => getAdminAlerts(page, search, perPage),
  });

  return (
    <div className='flex flex-col gap-4 p-4'>
      <div className='flex items-center justify-between gap-3'>
        <Text fw={700} size='xl'>{t('pages.admin.notifications.title', {})}</Text>
        <input
          className='rounded-md border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-3 py-2 text-sm'
          value={search ?? ''}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t('common.input.search', {})}
        />
      </div>

      {loading ? (
        <Spinner.Centered />
      ) : error ? (
        <Text c='red'>{String(error)}</Text>
      ) : !alerts || alerts.data.length === 0 ? (
        <Paper withBorder radius='md' className='p-4'>
          <EmptyState
            flush
            icon={faBell}
            title={t('pages.admin.notifications.empty.title', {})}
            description={t('pages.admin.notifications.empty.description', {})}
          />
        </Paper>
      ) : (
        <Paper withBorder radius='md' className='p-4'>
          <div className='flex flex-col gap-2'>
            {alerts.data.map((alert) => (
              <div
                key={alert.uuid}
                className='rounded-md border border-(--mantine-color-default-border) px-3 py-2'
              >
                <div className='flex flex-wrap items-center gap-2'>
                  <Badge
                    size='sm'
                    variant='light'
                    color={alert.kind === 'recovered' ? 'green' : 'red'}
                  >
                    {alert.kind}
                  </Badge>
                  <Text fw={600}>{alert.serverName}</Text>
                  {alert.serverUuidShort && (
                    <Text size='sm' c='dimmed'>`{alert.serverUuidShort}`</Text>
                  )}
                  <FormattedTimestamp timestamp={alert.detectedAt} />
                  {alert.oomKilled && (
                    <Badge size='xs' variant='light' color='red'>OOM</Badge>
                  )}
                  {alert.exitCode !== null && (
                    <Text size='sm' c='dimmed'>exit {alert.exitCode}</Text>
                  )}
                  {alert.crashId !== null && (
                    <Text size='sm' c='dimmed'>crash #{alert.crashId}</Text>
                  )}
                </div>
                {alert.lastConsole && (
                  <pre className='mt-2 max-h-32 overflow-y-auto whitespace-pre-wrap break-words rounded bg-(--mantine-color-default) p-2 font-mono text-xs'>
                    {alert.lastConsole}
                  </pre>
                )}
              </div>
            ))}
          </div>
          {alerts.total > alerts.data.length && (
            <div className='mt-3 flex justify-end gap-2'>
              <button
                type='button'
                className='rounded-md border border-(--mantine-color-default-border) px-3 py-1.5 text-sm'
                onClick={() => setPage(alerts.page + 1)}
              >
                {t('common.button.loadMore', {})}
              </button>
              <button
                type='button'
                className='rounded-md border border-(--mantine-color-default-border) px-3 py-1.5 text-sm'
                onClick={() => setPerPage(50)}
              >
                {t('common.button.showMore', {})}
              </button>
            </div>
          )}
        </Paper>
      )}
    </div>
  );
}
