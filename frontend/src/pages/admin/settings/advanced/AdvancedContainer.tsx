import { Alert } from '@mantine/core';
import { z } from 'zod';
import updateAdvancedSettings from '@/api/admin/settings/updateAdvancedSettings.ts';
import AdminSubContentContainer from '@/elements/containers/AdminSubContentContainer.tsx';
import { FormEngine, useFormEngine } from '@/elements/form-engine/index.ts';
import Group from '@/elements/layout/Group.tsx';
import { adminSettingsAdvancedSchema } from '@/lib/schemas/admin/settings.ts';
import { useFormDraft } from '@/plugins/form/useFormDraft.ts';
import { useHydrateForm } from '@/plugins/form/useHydrateForm.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useAdminStore } from '@/stores/admin.tsx';
import SettingsSaveButton from '../SettingsSaveButton.tsx';
import { useSettingsSection } from '../useSettingsSection.ts';
import EnvironmentSection from './EnvironmentSection.tsx';
import {
  advancedSettingsEmptyFormValues,
  advancedSettingsToFormValues,
  useAdvancedSettingsFormFields,
} from './advancedFormValues.tsx';

type AdvancedFormValues = z.infer<typeof adminSettingsAdvancedSchema>;

export default function AdvancedContainer() {
  const { t } = useTranslations();
  const advanced = useAdminStore((state) => state.advanced);

  const form = useFormEngine<AdvancedFormValues>('admin.settings.advanced', {
    schema: adminSettingsAdvancedSchema,
    initialValues: advancedSettingsEmptyFormValues,
  });

  useHydrateForm(form, advanced, advancedSettingsToFormValues);
  useFormDraft(form, 'advanced');

  const { loading, submit } = useSettingsSection({
    form,
    schema: adminSettingsAdvancedSchema,
    storeKey: 'advanced',
    update: updateAdvancedSettings,
    successMessage: t('pages.admin.settings.tabs.advanced.page.toast.updated', {}),
  });

  const fields = useAdvancedSettingsFormFields();

  return (
    <AdminSubContentContainer title={t('pages.admin.settings.tabs.advanced.page.title', {})} titleOrder={2}>
      <Alert color='yellow' title={t('pages.admin.settings.tabs.advanced.page.restartRequired.title', {})}>
        {t('pages.admin.settings.tabs.advanced.page.restartRequired.body', {})}
      </Alert>

      <form onSubmit={form.onSubmit(submit)}>
        <FormEngine form={form} fields={fields} />

        <Group mt='md'>
          <SettingsSaveButton loading={loading} disabled={!form.isValid()} />
        </Group>
      </form>

      <EnvironmentSection />
    </AdminSubContentContainer>
  );
}
