import { faPlay, faRotateRight, faSkull, faStop } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import type { z } from 'zod';
import { useShallow } from 'zustand/react/shallow';
import Button from '@/elements/buttons/Button.tsx';
import { ServerCan } from '@/elements/Can.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import { sendOrQueuePowerAction, useOfflineQueue } from '@/lib/console/offlineQueue.ts';
import { serverPowerAction } from '@/lib/schemas/server/server.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';

/**
 * Bottom action bar for the console page on phones (rendered only below
 * the `md` page breakpoint by ServerConsole; the infobar power controls
 * are hidden there instead, so the actions are not rendered twice).
 *
 * Large (`lg`, min 48px) Start / Stop / Restart buttons with safe-area
 * bottom padding, a confirm step for Stop — a stray tap on a phone should
 * not stop a live server — and queue-aware sending: when the websocket is
 * down the action is queued and replayed on reconnect instead of being
 * dropped (see `lib/console/offlineQueue.ts`).
 */
export default function MobileConsoleActionBar() {
  const { t } = useTranslations();
  const [confirmAction, setConfirmAction] = useState<'stop' | 'kill' | null>(null);
  const queued = useOfflineQueue();
  const { server, state, socketInstance, socketConnected } = useServerStore(
    useShallow((s) => ({
      server: s.server,
      state: s.state,
      socketInstance: s.socketInstance,
      socketConnected: s.socketConnected,
    })),
  );

  const killable = state === 'stopping';
  const blocked = !!server.status || server.isSuspended || server.isTransferring;
  // While disconnected the last known state may be stale, so only the
  // hard blocks apply; the action is queued rather than sent.
  const startDisabled = blocked || (!socketConnected ? false : state !== 'offline');
  const restartDisabled = blocked || (!socketConnected ? false : !state);
  const stopDisabled = blocked || (!socketConnected ? false : state === 'offline');

  const run = (action: z.infer<typeof serverPowerAction>) => {
    setConfirmAction(null);
    sendOrQueuePowerAction(socketInstance, socketConnected, action);
  };

  const onStopClick = () => {
    if (killable) {
      setConfirmAction('kill');
    } else {
      setConfirmAction('stop');
    }
  };

  useEffect(() => {
    if (state === 'offline') {
      setConfirmAction(null);
    }
  }, [state]);

  return (
    <div className='flex w-full flex-col gap-2'>
      <ConfirmationModal
        opened={confirmAction === 'stop'}
        onClose={() => setConfirmAction(null)}
        title={`Stop ${server.name}?`}
        confirm={t('common.enum.serverPowerAction.stop', {})}
        onConfirmed={() => run('stop')}
      >
        {`Are you sure you want to stop ${server.name}? Players will be disconnected.`}
      </ConfirmationModal>
      <ConfirmationModal
        opened={confirmAction === 'kill'}
        onClose={() => setConfirmAction(null)}
        title={t('pages.server.console.power.modal.forceStop.title', {})}
        confirm={t('common.button.continue', {})}
        onConfirmed={() => run('kill')}
      >
        {t('pages.server.console.power.modal.forceStop.content', {}).md()}
      </ConfirmationModal>

      <div className='flex w-full gap-2' role='group' aria-label={t('common.form.powerAction', {})}>
        <ServerCan action='control.start'>
          <Button
            color='green'
            size='lg'
            disabled={startDisabled}
            loading={state === 'starting'}
            onClick={() => run('start')}
            className='min-h-12 flex-1'
            leftSection={<FontAwesomeIcon icon={faPlay} size='sm' />}
          >
            {t('common.enum.serverPowerAction.start', {})}
          </Button>
        </ServerCan>
        <ServerCan action='control.restart'>
          <Button
            color='gray'
            size='lg'
            disabled={restartDisabled}
            onClick={() => run('restart')}
            className='min-h-12 flex-1'
            leftSection={<FontAwesomeIcon icon={faRotateRight} size='sm' />}
          >
            {t('common.enum.serverPowerAction.restart', {})}
          </Button>
        </ServerCan>
        <ServerCan action='control.stop'>
          <Button
            color='red'
            size='lg'
            disabled={stopDisabled}
            onClick={onStopClick}
            className='min-h-12 flex-1'
            leftSection={<FontAwesomeIcon icon={killable ? faSkull : faStop} size='sm' />}
          >
            {killable ? t('common.enum.serverPowerAction.kill', {}) : t('common.enum.serverPowerAction.stop', {})}
          </Button>
        </ServerCan>
      </div>

      {queued.length > 0 && (
        <p className='text-center text-xs text-(--mantine-color-dimmed)'>
          {socketConnected
            ? `${queued.length} queued action${queued.length === 1 ? '' : 's'} — sending now.`
            : `Queued — will send when reconnected (${queued.length})`}
        </p>
      )}
    </div>
  );
}
