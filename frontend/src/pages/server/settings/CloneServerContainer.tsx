import { FormEvent, useState } from 'react';
import { useNavigate } from 'react-router';
import cloneServer from '@/api/server/cloneServer.ts';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/buttons/Button.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Switch from '@/elements/input/Switch.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useServer } from '@/stores/server.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

/**
 * Client-side clone: only the server owner sees this (the settings page is
 * already gated by `settings.*` permissions; the API additionally requires
 * ownership). The clone keeps the same node and owner; allocations are
 * always fresh.
 */
export default function CloneServerContainer() {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const navigate = useNavigate();
  const server = useServer((state) => state.server);

  const [loading, setLoading] = useState(false);
  const [name, setName] = useState(`${server.name} (copy)`);
  const [includeFiles, setIncludeFiles] = useState(false);
  const [includeSchedules, setIncludeSchedules] = useState(true);

  if (!server.isOwner) return null;

  const doClone = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setLoading(true);
    cloneServer(server.uuid, {
      name: name.trim(),
      includeFiles,
      includeSchedules,
      startOnCompletion: false,
    })
      .then((clone) => {
        addToast(t('pages.server.settings.clone.toast.cloned', { name: clone.name }), 'success');
        navigate(`/server/${clone.uuidShort}`);
      })
      .catch((msg) => {
        addToast(httpErrorToHuman(msg), 'error');
      })
      .finally(() => setLoading(false));
  };

  return (
    <TitleCard title={t('pages.server.settings.clone.title', {})}>
      <form onSubmit={doClone}>
        <Stack>
          <Text size='sm'>{t('pages.server.settings.clone.description', {})}</Text>
          <TextInput
            withAsterisk
            label={t('common.form.serverName', {})}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <Switch
            label={t('pages.server.settings.clone.includeSchedules', {})}
            checked={includeSchedules}
            onChange={(e) => setIncludeSchedules(e.target.checked)}
          />
          <Switch
            label={t('pages.server.settings.clone.includeFiles', {})}
            checked={includeFiles}
            onChange={(e) => setIncludeFiles(e.target.checked)}
          />
          {includeFiles && (
            <Alert>{t('pages.server.settings.clone.filesCopyNote', {})}</Alert>
          )}
          <Button type='submit' disabled={!name.trim()} loading={loading}>
            {t('common.button.clone', {})}
          </Button>
        </Stack>
      </form>
    </TitleCard>
  );
}
