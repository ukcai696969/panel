import { useState } from 'react';
import getServerTemplates from '@/api/admin/server-templates/getServerTemplates.ts';
import getServerTemplate from '@/api/admin/server-templates/getServerTemplate.ts';
import { httpErrorToHuman } from '@/api/axios.ts';
import Select from '@/elements/input/Select.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useInfiniteSearchableResource } from '@/plugins/resource/useInfiniteSearchableResource.ts';
import { AdminServerTemplate } from '@/lib/schemas/serverTemplates.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

/**
 * Template picker for the admin server-create flow. Picking a template loads
 * its full snapshot and hands it to `onSelect`; the create page maps the
 * snapshot onto the form (egg, startup, image, limits, variables, name).
 * Templates carry no node/owner/allocation, so those stay blank for the
 * admin to choose — allocations are picked fresh when the server is created.
 */
export default function ServerTemplatePicker({ onSelect }: { onSelect: (template: AdminServerTemplate) => void }) {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const [loading, setLoading] = useState(false);

  const templates = useInfiniteSearchableResource<AdminServerTemplate>({
    queryKey: ['admin', 'server-templates'],
    fetcher: (page, search) => getServerTemplates(page, search),
  });

  return (
    <Stack>
      <Text size='sm'>{t('pages.admin.servers.tabs.general.page.card.template.description', {})}</Text>
      <Select
        label={t('pages.admin.servers.tabs.general.page.card.template.label', {})}
        placeholder={t('pages.admin.servers.tabs.general.page.card.template.placeholder', {})}
        data={templates.items.map((template) => ({
          label: `${template.name} — ${template.eggName}`,
          value: template.uuid,
        }))}
        searchable
        searchValue={templates.search}
        onSearchChange={templates.setSearch}
        infiniteScroll={templates.infiniteScroll}
        disabled={loading}
        onChange={(uuid) => {
          if (!uuid) return;
          setLoading(true);
          getServerTemplate(uuid)
            .then((template) => onSelect(template))
            .catch((err) => addToast(httpErrorToHuman(err), 'error'))
            .finally(() => setLoading(false));
        }}
      />
    </Stack>
  );
}
