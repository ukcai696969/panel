import { faArrowsRotate } from '@fortawesome/free-solid-svg-icons';
import { useForm } from '@mantine/form';
import { zod4Resolver } from 'mantine-form-zod-resolver';
import { useState } from 'react';
import { z } from 'zod';
import { httpErrorToHuman } from '@/api/axios.ts';
import updateRestartOnCrash from '@/api/server/settings/updateRestartOnCrash.ts';
import Button from '@/elements/buttons/Button.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Switch from '@/elements/input/Switch.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { serverSettingsRestartOnCrashSchema } from '@/lib/schemas/server/settings.ts';
import { liveValidation } from '@/plugins/form/liveValidation.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';

/**
 * "Restart on crash" per-server setting.
 *
 * The toggle only stores the flag (`servers.restart_on_crash`); the
 * backend watcher in `backend/src/restart_on_crash.rs` does the work —
 * up to 3 restarts with backoff (10s, 1m, 5m), then it stops and records
 * `server:crash.restart-exhausted` in the activity log. Alerting itself
 * belongs to the crash-alerts feature (Feature 1), which should subscribe
 * to the `server:crash.restart` / `server:crash.restart-exhausted`
 * activity events rather than this file duplicating webhook code.
 */
export default function RestartOnCrashContainer() {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const server = useServerStore((state) => state.server);
  const updateServer = useServerStore((state) => state.updateServer);

  const [loading, setLoading] = useState(false);

  const form = useForm<z.infer<typeof serverSettingsRestartOnCrashSchema>>({
    initialValues: {
      enabled: server.restartOnCrash,
    },
    ...liveValidation,
    validate: zod4Resolver(serverSettingsRestartOnCrashSchema),
  });

  const doUpdate = () => {
    setLoading(true);
    updateRestartOnCrash(server.uuid, form.values)
      .then(() => {
        addToast(t('pages.server.settings.restartOnCrash.toast.updated', {}), 'success');
        updateServer({ restartOnCrash: form.values.enabled });
      })
      .catch((msg) => {
        addToast(httpErrorToHuman(msg), 'error');
      })
      .finally(() => setLoading(false));
  };

  return (
    <TitleCard title={t('pages.server.settings.restartOnCrash.title', {})} icon={faArrowsRotate} className='h-full order-25'>
      <form onSubmit={form.onSubmit(() => doUpdate())} className='h-full'>
        <Stack h='100%'>
          <Switch label={t('common.form.enabled', {})} {...form.getInputProps('enabled', { type: 'checkbox' })} />
          <Text size='sm' c='dimmed'>
            {t('pages.server.settings.restartOnCrash.description', {})}
          </Text>

          <Group mt='auto'>
            <Button type='submit' loading={loading} disabled={!form.isValid()}>
              {t('common.button.save', {})}
            </Button>
          </Group>
        </Stack>
      </form>
    </TitleCard>
  );
}
