import { FormEvent, useState } from 'react';
import createServerTemplate from '@/api/server/server-templates/createServerTemplate.ts';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/buttons/Button.tsx';
import TextArea from '@/elements/input/TextArea.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useServer } from '@/stores/server.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

/** Save this server's configuration as a personal reusable template. */
export default function SaveAsTemplateContainer() {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const server = useServer((state) => state.server);

  const [loading, setLoading] = useState(false);
  const [name, setName] = useState(server.name);
  const [description, setDescription] = useState('');

  if (!server.isOwner) return null;

  const doSave = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setLoading(true);
    createServerTemplate({
      serverUuid: server.uuid,
      name: name.trim(),
      description: description.trim() || undefined,
    })
      .then((template) => {
        addToast(t('pages.server.settings.saveAsTemplate.toast.saved', { name: template.name }), 'success');
      })
      .catch((msg) => {
        addToast(httpErrorToHuman(msg), 'error');
      })
      .finally(() => setLoading(false));
  };

  return (
    <TitleCard title={t('pages.server.settings.saveAsTemplate.title', {})}>
      <form onSubmit={doSave}>
        <Stack>
          <Text size='sm'>{t('pages.server.settings.saveAsTemplate.description', {})}</Text>
          <TextInput
            withAsterisk
            label={t('common.form.name', {})}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <TextArea
            label={t('common.form.description', {})}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <Button type='submit' disabled={!name.trim()} loading={loading}>
            {t('common.button.save', {})}
          </Button>
        </Stack>
      </form>
    </TitleCard>
  );
}
