import { faBell, faSkull, faTrashCan, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import createWebhook from '@/api/server/webhooks/createWebhook.ts';
import deleteWebhook from '@/api/server/webhooks/deleteWebhook.ts';
import getServerAlerts from '@/api/server/webhooks/getServerAlerts.ts';
import getWebhooks from '@/api/server/webhooks/getWebhooks.ts';
import { ServerCan } from '@/elements/Can.tsx';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import Button from '@/elements/buttons/Button.tsx';
import ServerContentContainer from '@/elements/containers/ServerContentContainer.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import EmptyState from '@/elements/feedback/EmptyState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Paper from '@/elements/layout/Paper.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import FormattedTimestamp from '@/elements/time/FormattedTimestamp.tsx';
import Text from '@/elements/typography/Text.tsx';
import { queryKeys } from '@/lib/queryKeys.ts';
import { useResource } from '@/plugins/resource/useResource.ts';
import { useSearchablePaginatedTable } from '@/plugins/resource/useSearchablePaginatedTable.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';

const ALL_EVENTS = ['server_crashed', 'server_offline', 'server_recovered'] as const;
type WebhookEvent = (typeof ALL_EVENTS)[number];

function alertKindLabel(kind: string) {
  switch (kind) {
    case 'crashed':
      return 'Crashed';
    case 'offline':
      return 'Went offline';
    case 'recovered':
      return 'Recovered';
    default:
      return kind;
  }
}

export default function ServerAlerts() {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const queryClient = useQueryClient();
  const server = useServerStore((state) => state.server);

  const [showForm, setShowForm] = useState(false);
  const [name, setName] = useState('');
  const [kind, setKind] = useState<'discord' | 'generic'>('discord');
  const [url, setUrl] = useState('');
  const [events, setEvents] = useState<WebhookEvent[]>(['server_crashed']);
  const [creating, setCreating] = useState(false);
  const [createdSecret, setCreatedSecret] = useState<string | null>(null);
  const [deletingUuid, setDeletingUuid] = useState<string | null>(null);

  const {
    data: webhooks,
    error: webhooksError,
    refetch: refetchWebhooks,
  } = useResource({
    queryKey: queryKeys.server(server.uuid).webhooks.all(),
    queryFn: () => getWebhooks(server.uuid, 1, undefined, 100),
  });

  const {
    data: alerts,
    loading: alertsLoading,
    error: alertsError,
    search,
    setSearch,
    setPage,
    setPerPage,
  } = useSearchablePaginatedTable({
    queryKey: queryKeys.server(server.uuid).alerts.all(),
    fetcher: (page, search, perPage) => getServerAlerts(server.uuid, page, search, perPage),
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: queryKeys.server(server.uuid).webhooks.all() });
    queryClient.invalidateQueries({ queryKey: queryKeys.server(server.uuid).alerts.all() });
  };

  const toggleEvent = (event: WebhookEvent) =>
    setEvents((current) =>
      current.includes(event) ? current.filter((e) => e !== event) : [...current, event],
    );

  const doCreate = async () => {
    if (!name.trim() || !url.trim() || events.length === 0) return;
    setCreating(true);
    setCreatedSecret(null);
    await createWebhook(server.uuid, {
      name: name.trim(),
      kind,
      url: url.trim(),
      events,
      enabled: true,
    })
      .then(({ secret }) => {
        if (secret) setCreatedSecret(secret);
        setName('');
        setUrl('');
        setEvents(['server_crashed']);
        setShowForm(false);
        addToast(t('pages.server.alerts.toast.created', {}), 'success');
        invalidate();
      })
      .catch((msg) => addToast(httpErrorToHuman(msg), 'error'))
      .finally(() => setCreating(false));
  };

  const doDelete = async () => {
    if (!deletingUuid) return;
    await deleteWebhook(server.uuid, deletingUuid)
      .then(() => {
        setDeletingUuid(null);
        addToast(t('pages.server.alerts.toast.deleted', {}), 'success');
        invalidate();
      })
      .catch((msg) => addToast(httpErrorToHuman(msg), 'error'));
  };

  return (
    <ServerContentContainer
      title={t('pages.server.alerts.title', {})}
      search={search}
      setSearch={setSearch}
    >
      <ConfirmationModal
        opened={deletingUuid !== null}
        onClose={() => setDeletingUuid(null)}
        title={t('common.modal.deleteWebhook.title', {})}
        confirm={t('common.button.delete', {})}
        onConfirmed={doDelete}
      >
        {t('common.modal.deleteWebhook.content', {}).md()}
      </ConfirmationModal>

      {createdSecret && (
        <Paper withBorder radius='md' className='mb-4 p-4'>
          <Text fw={600}>{t('pages.server.alerts.secret.title', {})}</Text>
          <Text size='sm' c='dimmed'>
            {t('pages.server.alerts.secret.description', {})}
          </Text>
          <code className='mt-2 block break-all rounded bg-(--mantine-color-default) p-2 font-mono text-sm'>
            {createdSecret}
          </code>
          <div className='mt-2 flex justify-end'>
            <Button variant='default' onClick={() => setCreatedSecret(null)}>
              {t('common.button.done', {})}
            </Button>
          </div>
        </Paper>
      )}

      <Paper withBorder radius='md' className='mb-4 p-4'>
        <div className='flex items-center justify-between gap-3'>
          <Text fw={600}>{t('pages.server.alerts.webhooks.title', {})}</Text>
          <ServerCan action='webhooks.create'>
            <Button variant='default' onClick={() => setShowForm((v) => !v)}>
              {t('pages.server.alerts.webhooks.button.add', {})}
            </Button>
          </ServerCan>
        </div>

        {showForm && (
          <div className='mt-4 flex flex-col gap-3'>
            <label className='flex flex-col gap-1'>
              <Text size='sm' fw={600}>{t('pages.server.alerts.webhooks.field.name', {})}</Text>
              <input
                className='rounded-md border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-3 py-2 text-sm'
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder='e.g. Discord #server-alerts'
                maxLength={255}
              />
            </label>
            <label className='flex flex-col gap-1'>
              <Text size='sm' fw={600}>{t('pages.server.alerts.webhooks.field.kind', {})}</Text>
              <select
                className='rounded-md border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-3 py-2 text-sm'
                value={kind}
                onChange={(e) => setKind(e.target.value as 'discord' | 'generic')}
              >
                <option value='discord'>Discord</option>
                <option value='generic'>Generic (signed JSON)</option>
              </select>
            </label>
            <label className='flex flex-col gap-1'>
              <Text size='sm' fw={600}>{t('pages.server.alerts.webhooks.field.url', {})}</Text>
              <input
                className='rounded-md border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-3 py-2 text-sm'
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder={kind === 'discord' ? 'https://discord.com/api/webhooks/…' : 'https://…'}
                inputMode='url'
              />
              <Text size='xs' c='dimmed'>
                {t('pages.server.alerts.webhooks.field.urlHint', {})}
              </Text>
            </label>
            <div className='flex flex-wrap gap-4'>
              {ALL_EVENTS.map((event) => (
                <label key={event} className='flex items-center gap-2 text-sm'>
                  <input type='checkbox' checked={events.includes(event)} onChange={() => toggleEvent(event)} />
                  {event.replace('server_', '').replace('_', ' ')}
                </label>
              ))}
            </div>
            <div className='flex gap-2'>
              <Button loading={creating} onClick={doCreate}>
                {t('common.button.create', {})}
              </Button>
              <Button variant='default' onClick={() => setShowForm(false)}>
                {t('common.button.cancel', {})}
              </Button>
            </div>
          </div>
        )}

        <div className='mt-4 flex flex-col gap-2'>
          {!webhooks ? (
            <Spinner />
          ) : webhooksError ? (
            <Text c='red'>{httpErrorToHuman(webhooksError)}</Text>
          ) : webhooks.data.length === 0 ? (
            <EmptyState
              flush
              icon={faBell}
              title={t('pages.server.alerts.webhooks.empty.title', {})}
              description={t('pages.server.alerts.webhooks.empty.description', {})}
            />
          ) : (
            webhooks.data.map((webhook) => (
              <div
                key={webhook.uuid}
                className='flex items-center justify-between gap-3 rounded-md border border-(--mantine-color-default-border) px-3 py-2'
              >
                <div className='min-w-0'>
                  <Text fw={600} className='truncate'>{webhook.name}</Text>
                  <Text size='sm' c='dimmed' className='truncate'>
                    {webhook.kind} · {webhook.url} · {webhook.events.join(', ')}
                  </Text>
                  {webhook.failureCount > 0 && (
                    <Badge size='xs' variant='light' color='red'>
                      {t('pages.server.alerts.webhooks.failures', { count: webhook.failureCount })}
                    </Badge>
                  )}
                </div>
                <div className='flex items-center gap-1'>
                  {!webhook.enabled && (
                    <Badge size='xs' variant='light' color='gray'>
                      {t('common.disabled', {})}
                    </Badge>
                  )}
                  <ServerCan action='webhooks.delete'>
                    <Tooltip label={t('common.button.delete', {})}>
                      <ActionIcon
                        variant='subtle'
                        color='red'
                        aria-label={t('common.button.delete', {})}
                        onClick={() => setDeletingUuid(webhook.uuid)}
                      >
                        <FontAwesomeIcon icon={faTrashCan} />
                      </ActionIcon>
                    </Tooltip>
                  </ServerCan>
                </div>
              </div>
            ))
          )}
          {webhooks && webhooks.data.length > 0 && (
            <Button variant='subtle' size='xs' onClick={() => refetchWebhooks()}>
              {t('common.button.refresh', {})}
            </Button>
          )}
        </div>
      </Paper>

      <Paper withBorder radius='md' className='p-4'>
        <Text fw={600} className='mb-3'>{t('pages.server.alerts.history.title', {})}</Text>
        {alertsLoading ? (
          <Spinner />
        ) : alertsError ? (
          <Text c='red'>{httpErrorToHuman(alertsError)}</Text>
        ) : !alerts || alerts.data.length === 0 ? (
          <EmptyState
            flush
            icon={faSkull}
            title={t('pages.server.alerts.history.empty.title', {})}
            description={t('pages.server.alerts.history.empty.description', {})}
          />
        ) : (
          <div className='flex flex-col gap-2'>
            {alerts.data.map((alert) => (
              <div
                key={alert.uuid}
                className='rounded-md border border-(--mantine-color-default-border) px-3 py-2'
              >
                <div className='flex flex-wrap items-center gap-2'>
                  <Badge size='sm' variant='light' color={alert.kind === 'recovered' ? 'green' : 'red'}>
                    {alertKindLabel(alert.kind)}
                  </Badge>
                  <FormattedTimestamp timestamp={alert.detectedAt} />
                  {alert.oomKilled && (
                    <Badge size='xs' variant='light' color='red'>
                      {t('pages.server.console.crashes.badge.oomKilled', {})}
                    </Badge>
                  )}
                  {alert.exitCode !== null && (
                    <Text size='sm' c='dimmed'>exit {alert.exitCode}</Text>
                  )}
                  {alert.crashId !== null && (
                    <Text size='sm' c='dimmed'>crash #{alert.crashId}</Text>
                  )}
                </div>
                {alert.lastConsole && (
                  <pre className='mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded bg-(--mantine-color-default) p-2 font-mono text-xs'>
                    {alert.lastConsole}
                  </pre>
                )}
              </div>
            ))}
            {alerts.total > alerts.data.length && (
              <div className='flex justify-end gap-2'>
                <Button variant='default' size='xs' onClick={() => setPage(alerts.page + 1)}>
                  {t('common.button.loadMore', {})}
                </Button>
                <Button variant='subtle' size='xs' onClick={() => setPerPage(50)}>
                  {t('common.button.showMore', {})}
                </Button>
              </div>
            )}
          </div>
        )}
      </Paper>

      <Text size='xs' c='dimmed' className='mt-3 flex items-center gap-1'>
        <FontAwesomeIcon icon={faTriangleExclamation} />
        {t('pages.server.alerts.note', {})}
      </Text>
    </ServerContentContainer>
  );
}
