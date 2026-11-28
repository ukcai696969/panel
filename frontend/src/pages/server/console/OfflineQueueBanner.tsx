import { faClockRotateLeft } from '@fortawesome/free-solid-svg-icons';
import Button from '@/elements/buttons/Button.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import { describeQueuedItem, offlineActionQueue, useOfflineQueue } from '@/lib/console/offlineQueue.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';

/**
 * Shown on the console page while actions are waiting for the websocket to
 * come back. The queue itself lives only in memory (see
 * `lib/console/offlineQueue.ts`); this banner is the visible half of it.
 */
export default function OfflineQueueBanner() {
  const { t } = useTranslations();
  const queued = useOfflineQueue();
  const socketConnected = useServerStore((state) => state.socketConnected);

  if (queued.length === 0) {
    return null;
  }

  const summary = queued.map(describeQueuedItem).join(', ');

  return (
    <Alert icon={faClockRotateLeft} color='yellow' className='mb-4'>
      <div className='flex flex-wrap items-center justify-between gap-2'>
        <span className='min-w-0'>
          {socketConnected
            ? `${queued.length} queued action${queued.length === 1 ? '' : 's'} — sending now.`
            : `Queued — will send when reconnected (${queued.length}): ${summary}`}
        </span>
        <Button
          size='xs'
          color='yellow'
          variant='light'
          className='min-h-11 shrink-0'
          onClick={() => offlineActionQueue.clear()}
        >
          {t('common.button.discard', {})}
        </Button>
      </div>
    </Alert>
  );
}
