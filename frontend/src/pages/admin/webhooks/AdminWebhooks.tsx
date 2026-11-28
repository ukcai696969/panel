import createWebhook from '@/api/admin/webhooks/createWebhook.ts';
import deleteWebhook from '@/api/admin/webhooks/deleteWebhook.ts';
import getWebhookDeliveries from '@/api/admin/webhooks/getWebhookDeliveries.ts';
import getWebhooks from '@/api/admin/webhooks/getWebhooks.ts';
import rotateWebhookSecret from '@/api/admin/webhooks/rotateWebhookSecret.ts';
import testWebhook from '@/api/admin/webhooks/testWebhook.ts';
import updateWebhook from '@/api/admin/webhooks/updateWebhook.ts';
import Button from '@/elements/buttons/Button.tsx';
import AdminContentContainer from '@/elements/containers/AdminContentContainer.tsx';
import Table, { TableData, TableRow } from '@/elements/data-display/Table.tsx';
import { WEBHOOK_EVENTS, webhookSchema } from '@/lib/schemas/admin/webhooks.ts';
import { useSearchablePaginatedTable } from '@/plugins/resource/useSearchablePaginatedTable.ts';
import { MultiSelect, Switch, TextInput } from '@mantine/core';
import { useEffect, useState } from 'react';
import type { z } from 'zod';

// NOTE: webhook-specific labels here are plain English literals. Add
// `pages.admin.webhooks.*` keys to frontend/src/translations.ts and switch
// these to t() calls when the page is wired into the translation pass.
type Webhook = z.infer<typeof webhookSchema>;

export default function AdminWebhooks() {
  const [refreshKey, setRefreshKey] = useState(0);
  const {
    data: webhooks,
    loading,
    error,
    search,
    setSearch,
    setPage,
    setPerPage,
  } = useSearchablePaginatedTable({
    queryKey: ['admin', 'webhooks', refreshKey],
    fetcher: (page, search, perPage) => getWebhooks(page, search, perPage),
  });

  const [editing, setEditing] = useState<Webhook | null>(null);
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [events, setEvents] = useState<string[]>([]);
  const [enabled, setEnabled] = useState(true);
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  // Shown exactly once after create/rotate; never fetched from the API again.
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null);
  const [secretCopied, setSecretCopied] = useState(false);

  const [selectedUuid, setSelectedUuid] = useState<string | null>(null);
  const [deliveries, setDeliveries] = useState<Awaited<
    ReturnType<typeof getWebhookDeliveries>
  > | null>(null);
  const [deliveriesError, setDeliveriesError] = useState<string | null>(null);

  const [testResult, setTestResult] = useState<{ uuid: string; text: string } | null>(null);
  const [deleteArmed, setDeleteArmed] = useState<string | null>(null);

  useEffect(() => {
    if (!selectedUuid) {
      setDeliveries(null);
      return;
    }

    let cancelled = false;
    setDeliveriesError(null);
    getWebhookDeliveries(selectedUuid, 1, 10)
      .then((result) => {
        if (!cancelled) setDeliveries(result);
      })
      .catch(() => {
        if (!cancelled) setDeliveriesError('Could not load deliveries.');
      });
    return () => {
      cancelled = true;
    };
  }, [selectedUuid, refreshKey]);

  const startCreate = () => {
    setEditing(null);
    setName('');
    setUrl('');
    setEvents([]);
    setEnabled(true);
    setFormError(null);
    setRevealedSecret(null);
  };

  const startEdit = (webhook: Webhook) => {
    setEditing(webhook);
    setName(webhook.name);
    setUrl(webhook.url);
    setEvents([...webhook.events]);
    setEnabled(webhook.enabled);
    setFormError(null);
    setRevealedSecret(null);
  };

  const save = async () => {
    setSaving(true);
    setFormError(null);
    try {
      if (editing) {
        await updateWebhook(editing.uuid, { name, url, events, enabled });
      } else {
        const created = await createWebhook({ name, url, events, enabled });
        // Keep the revealed secret visible: reset the form without clearing it.
        setRevealedSecret(created.secret);
        setSecretCopied(false);
      }
      setRefreshKey((key) => key + 1);
      setEditing(null);
      setName('');
      setUrl('');
      setEvents([]);
      setEnabled(true);
      setFormError(null);
    } catch {
      setFormError('Could not save the webhook. Check the URL and events and try again.');
    } finally {
      setSaving(false);
    }
  };

  const sendTest = async (webhook: Webhook) => {
    try {
      const result = await testWebhook(webhook.uuid);
      setTestResult({
        uuid: webhook.uuid,
        text: result.success
          ? `Test delivery succeeded (HTTP ${result.statusCode ?? '?'}, ${result.attempts} attempt(s)).`
          : `Test delivery failed${result.statusCode ? ` (HTTP ${result.statusCode})` : ''}: ${result.error ?? 'unknown error'}`,
      });
      setRefreshKey((key) => key + 1);
    } catch {
      setTestResult({ uuid: webhook.uuid, text: 'Test delivery could not be started.' });
    }
  };

  const rotateSecret = async (webhook: Webhook) => {
    try {
      setRevealedSecret(await rotateWebhookSecret(webhook.uuid));
      setSecretCopied(false);
    } catch {
      setFormError('Could not rotate the secret. Try again.');
    }
  };

  const remove = async (webhook: Webhook) => {
    if (deleteArmed !== webhook.uuid) {
      setDeleteArmed(webhook.uuid);
      return;
    }
    setDeleteArmed(null);
    await deleteWebhook(webhook.uuid);
    if (selectedUuid === webhook.uuid) setSelectedUuid(null);
    setRefreshKey((key) => key + 1);
  };

  const copySecret = async () => {
    if (!revealedSecret) return;
    try {
      await navigator.clipboard.writeText(revealedSecret);
      setSecretCopied(true);
    } catch {
      // Clipboard unavailable; the secret is still visible above.
    }
  };

  return (
    <AdminContentContainer title='Webhooks' search={search} setSearch={setSearch}>
      {revealedSecret && (
        <div className='mb-4 rounded border border-(--mantine-color-yellow-outline) bg-(--mantine-color-yellow-light) p-3'>
          <p className='font-medium'>Signing secret — copy it now, it will not be shown again.</p>
          <code className='block break-all mt-1'>{revealedSecret}</code>
          <Button variant='outline' size='xs' className='mt-2' onClick={copySecret}>
            {secretCopied ? 'Copied' : 'Copy secret'}
          </Button>
        </div>
      )}

      <div className='mb-4 rounded border border-(--mantine-color-default-border) p-3 flex flex-col gap-3 max-w-2xl'>
        <TextInput label='Name' value={name} onChange={(event) => setName(event.target.value)} />
        <TextInput
          label='URL'
          placeholder='https://example.com/panel-webhook'
          value={url}
          onChange={(event) => setUrl(event.target.value)}
        />
        <MultiSelect
          label='Events'
          data={[...WEBHOOK_EVENTS]}
          value={events}
          onChange={setEvents}
          searchable
        />
        <Switch label='Enabled' checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />
        {formError && <p className='text-sm text-(--mantine-color-error)'>{formError}</p>}
        <div className='flex gap-2'>
          <Button onClick={save} loading={saving} disabled={!name || !url || events.length === 0}>
            {editing ? 'Save webhook' : 'Create webhook'}
          </Button>
          {editing && (
            <Button variant='outline' onClick={startCreate}>
              Cancel edit
            </Button>
          )}
        </div>
      </div>

      <Table
        columns={['Name', 'URL', 'Events', 'Enabled', 'Created', '']}
        loading={loading}
        error={error}
        pagination={webhooks}
        onPageSelect={setPage}
        onPerPageSelect={setPerPage}
      >
        {webhooks?.data.map((webhook) => (
          <TableRow key={webhook.uuid}>
            <TableData>{webhook.name}</TableData>
            <TableData className='max-w-64 truncate'>{webhook.url}</TableData>
            <TableData className='max-w-96 truncate'>{webhook.events.join(', ')}</TableData>
            <TableData>{webhook.enabled ? 'Yes' : 'No'}</TableData>
            <TableData>{webhook.created.toLocaleString()}</TableData>
            <TableData>
              <div className='flex flex-wrap gap-1'>
                <Button variant='outline' size='xs' onClick={() => startEdit(webhook)}>
                  Edit
                </Button>
                <Button variant='outline' size='xs' onClick={() => sendTest(webhook)}>
                  Test
                </Button>
                <Button variant='outline' size='xs' onClick={() => rotateSecret(webhook)}>
                  Rotate secret
                </Button>
                <Button
                  variant='outline'
                  size='xs'
                  onClick={() =>
                    setSelectedUuid(selectedUuid === webhook.uuid ? null : webhook.uuid)
                  }
                >
                  Deliveries
                </Button>
                <Button
                  variant='outline'
                  size='xs'
                  color='red'
                  onClick={() => remove(webhook)}
                >
                  {deleteArmed === webhook.uuid ? 'Confirm delete?' : 'Delete'}
                </Button>
              </div>
              {testResult?.uuid === webhook.uuid && (
                <p className='text-xs mt-1'>{testResult.text}</p>
              )}
            </TableData>
          </TableRow>
        ))}
      </Table>

      {selectedUuid && (
        <div className='mt-4'>
          <h2 className='text-lg font-medium mb-2'>Recent deliveries</h2>
          {deliveriesError && <p className='text-sm text-(--mantine-color-error)'>{deliveriesError}</p>}
          {deliveries && (
            <Table
              columns={['Event', 'Attempt', 'Status', 'Result', 'Error', 'When']}
              pagination={deliveries}
              onPageSelect={() => {}}
            >
              {deliveries.data.map((delivery) => (
                <TableRow key={delivery.uuid}>
                  <TableData>{delivery.event}</TableData>
                  <TableData>{delivery.attempt}</TableData>
                  <TableData>{delivery.statusCode ?? '—'}</TableData>
                  <TableData>{delivery.success ? 'Delivered' : 'Failed'}</TableData>
                  <TableData className='max-w-96 truncate'>{delivery.error ?? '—'}</TableData>
                  <TableData>{delivery.created.toLocaleString()}</TableData>
                </TableRow>
              ))}
            </Table>
          )}
        </div>
      )}
    </AdminContentContainer>
  );
}
