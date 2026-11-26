import { ModalProps } from '@mantine/core';
import { FormEvent, useEffect, useState } from 'react';
import { useNavigate } from 'react-router';
import cloneServer from '@/api/admin/servers/cloneServer.ts';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/buttons/Button.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Switch from '@/elements/input/Switch.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import FormModal from '@/elements/modals/FormModal.tsx';
import { ModalFooter } from '@/elements/modals/Modal.tsx';
import Text from '@/elements/typography/Text.tsx';
import { AdminServer } from '@/lib/schemas/admin/servers.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

export default function ServerCloneModal({ server, ...props }: ModalProps & { server: AdminServer }) {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const navigate = useNavigate();

  const [loading, setLoading] = useState(false);
  const [name, setName] = useState(`${server.name} (copy)`);
  const [includeFiles, setIncludeFiles] = useState(false);
  const [includeSchedules, setIncludeSchedules] = useState(true);
  const [startOnCompletion, setStartOnCompletion] = useState(false);

  useEffect(() => {
    if (props.opened) {
      setName(`${server.name} (copy)`);
      setIncludeFiles(false);
      setIncludeSchedules(true);
      setStartOnCompletion(false);
    }
  }, [props.opened, server.name]);

  const doClone = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setLoading(true);
    cloneServer(server.uuid, {
      name: name.trim(),
      includeFiles,
      includeSchedules,
      startOnCompletion,
    })
      .then((clone) => {
        addToast(t('pages.admin.servers.tabs.management.page.clone.toast.cloned', { name: clone.name }), 'success');
        props.onClose();
        navigate(`/admin/servers/${clone.uuidShort}`);
      })
      .catch((msg) => {
        addToast(httpErrorToHuman(msg), 'error');
      })
      .finally(() => setLoading(false));
  };

  return (
    <FormModal
      title={t('pages.admin.servers.tabs.management.page.clone.modal.title', {})}
      loading={loading}
      {...props}
      onSubmit={doClone}
    >
      <Stack>
        <Text size='sm'>
          {t('pages.admin.servers.tabs.management.page.clone.modal.description', { name: server.name }).md()}
        </Text>

        <TextInput
          withAsterisk
          label={t('common.form.serverName', {})}
          placeholder={t('common.form.serverName', {})}
          value={name}
          onChange={(e) => setName(e.target.value)}
        />

        <Switch
          label={t('pages.admin.servers.tabs.management.page.clone.modal.form.includeSchedules', {})}
          description={t('pages.admin.servers.tabs.management.page.clone.modal.form.includeSchedulesDescription', {})}
          checked={includeSchedules}
          onChange={(e) => setIncludeSchedules(e.target.checked)}
        />

        <Switch
          label={t('pages.admin.servers.tabs.management.page.clone.modal.form.includeFiles', {})}
          description={t('pages.admin.servers.tabs.management.page.clone.modal.form.includeFilesDescription', {})}
          checked={includeFiles}
          onChange={(e) => setIncludeFiles(e.target.checked)}
        />

        {includeFiles && (
          <Alert>
            {t('pages.admin.servers.tabs.management.page.clone.modal.alert.filesCopy', {})}
          </Alert>
        )}

        <Switch
          label={t('pages.admin.servers.tabs.management.page.clone.modal.form.startOnCompletion', {})}
          checked={startOnCompletion}
          onChange={(e) => setStartOnCompletion(e.target.checked)}
        />
      </Stack>

      <ModalFooter>
        <Button type='submit' disabled={!name.trim()} loading={loading}>
          {t('common.button.clone', {})}
        </Button>
        <Button variant='default' onClick={() => props.onClose()}>
          {t('common.button.cancel', {})}
        </Button>
      </ModalFooter>
    </FormModal>
  );
}
