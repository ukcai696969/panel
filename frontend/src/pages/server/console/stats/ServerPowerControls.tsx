import { faPlay, faRotateRight, faSkull, faStop } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import { z } from 'zod';
import { useShallow } from 'zustand/react/shallow';
import Button from '@/elements/buttons/Button.tsx';
import { ServerCan } from '@/elements/Can.tsx';
import ExtensionSlot from '@/elements/ExtensionSlot.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import { sendOrQueuePowerAction } from '@/lib/console/offlineQueue.ts';
import { serverPowerAction } from '@/lib/schemas/server/server.ts';
import { usePageBreakpoint } from '@/plugins/viewport/usePageBreakpoint.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';

export default function ServerPowerControls() {
  const { t } = useTranslations();
  // `null` = no confirm open; Stop now asks for confirmation too, so a stray
  // tap (or click) cannot stop a live server. Kill keeps its own modal.
  const [confirmAction, setConfirmAction] = useState<'stop' | 'kill' | null>(null);
  const isMobile = !usePageBreakpoint('md');
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

  // While the websocket is down the buttons stay enabled and the action is
  // queued for replay on reconnect (see lib/console/offlineQueue.ts). The
  // last known power state may be stale while disconnected, so state-based
  // gating is skipped then; the hard blocks still apply.
  const startDisabled = blocked || (!socketConnected ? false : state !== 'offline');
  const restartDisabled = blocked || (!socketConnected ? false : !state);
  const stopDisabled = blocked || (!socketConnected ? false : state === 'offline');

  const run = (action: z.infer<typeof serverPowerAction> | 'kill-confirmed') => {
    setConfirmAction(null);
    sendOrQueuePowerAction(socketInstance, socketConnected, action === 'kill-confirmed' ? 'kill' : action);
  };

  const onButtonClick = (action: z.infer<typeof serverPowerAction> | 'kill-confirmed') => {
    if (action === 'kill') {
      return setConfirmAction('kill');
    }
    if (action === 'stop') {
      return setConfirmAction('stop');
    }

    run(action);
  };

  useEffect(() => {
    if (state === 'offline') {
      setConfirmAction(null);
    }
  }, [state]);

  return (
    <div className='flex w-full md:w-fit gap-2'>
      <ConfirmationModal
        opened={confirmAction === 'kill'}
        onClose={() => setConfirmAction(null)}
        title={t('pages.server.console.power.modal.forceStop.title', {})}
        confirm={t('common.button.continue', {})}
        onConfirmed={() => run('kill-confirmed')}
      >
        {t('pages.server.console.power.modal.forceStop.content', {}).md()}
      </ConfirmationModal>

      <ConfirmationModal
        opened={confirmAction === 'stop'}
        onClose={() => setConfirmAction(null)}
        title={`Stop ${server.name}?`}
        confirm={t('common.enum.serverPowerAction.stop', {})}
        onConfirmed={() => run('stop')}
      >
        {`Are you sure you want to stop ${server.name}? Players will be disconnected.`}
      </ConfirmationModal>

      <ExtensionSlot
        components={
          window.extensionContext.extensionRegistry.pages.server.console.powerButtonComponents.prependedComponents
        }
        name='console-powerbutton-prepended'
      />

      <ServerCan action='control.start'>
        <Button
          color='green'
          size={isMobile ? 'lg' : 'md'}
          disabled={startDisabled}
          loading={state === 'starting'}
          onClick={() => onButtonClick('start')}
          className='min-h-12 md:min-h-0 flex-1 min-w-fit'
          leftSection={<FontAwesomeIcon icon={faPlay} size='sm' />}
        >
          {t('common.enum.serverPowerAction.start', {})}
        </Button>
      </ServerCan>
      <ServerCan action='control.restart'>
        <Button
          color='gray'
          size={isMobile ? 'lg' : 'md'}
          disabled={restartDisabled}
          onClick={() => onButtonClick('restart')}
          className='min-h-12 md:min-h-0 flex-1 min-w-fit'
          leftSection={<FontAwesomeIcon icon={faRotateRight} size='sm' />}
        >
          {t('common.enum.serverPowerAction.restart', {})}
        </Button>
      </ServerCan>
      <ServerCan action='control.stop'>
        <Button
          color='red'
          size={isMobile ? 'lg' : 'md'}
          disabled={stopDisabled}
          onClick={() => onButtonClick(killable ? 'kill' : 'stop')}
          className='min-h-12 md:min-h-0 flex-1 min-w-fit'
          leftSection={<FontAwesomeIcon icon={killable ? faSkull : faStop} size='sm' />}
        >
          {killable ? t('common.enum.serverPowerAction.kill', {}) : t('common.enum.serverPowerAction.stop', {})}
        </Button>
      </ServerCan>

      <ExtensionSlot
        components={
          window.extensionContext.extensionRegistry.pages.server.console.powerButtonComponents.appendedComponents
        }
        name='console-powerButton-appended'
      />
    </div>
  );
}
