import getAdminActivity from '@/api/admin/getAdminActivity.ts';
import getAdminActivityExport from '@/api/admin/getAdminActivityExport.ts';
import ActivityRow from '@/elements/activity/ActivityRow.tsx';
import ClearUserFilterButton from '@/elements/activity/ClearUserFilterButton.tsx';
import Button from '@/elements/buttons/Button.tsx';
import AdminContentContainer from '@/elements/containers/AdminContentContainer.tsx';
import Table from '@/elements/data-display/Table.tsx';
import { queryKeys } from '@/lib/queryKeys.ts';
import { activityColumns } from '@/lib/tableColumns.ts';
import { useSearchablePaginatedTable } from '@/plugins/resource/useSearchablePaginatedTable.ts';
import { useUserFilter } from '@/plugins/useUserFilter.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useState } from 'react';

export default function AdminActivity() {
  const { t } = useTranslations();
  const { filterUserUuid, setFilterUserUuid } = useUserFilter();

  const {
    data: activities,
    loading,
    error,
    search,
    setSearch,
    setPage,
    setPerPage,
  } = useSearchablePaginatedTable({
    queryKey: queryKeys.admin.activity.all(filterUserUuid),
    fetcher: (page, search, perPage) => getAdminActivity(filterUserUuid, page, search, perPage),
  });

  const [exportFormat, setExportFormat] = useState<'csv' | 'json'>('csv');
  const [exportFrom, setExportFrom] = useState('');
  const [exportTo, setExportTo] = useState('');
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);

  const exportActivity = async () => {
    setExporting(true);
    setExportError(null);
    try {
      await getAdminActivityExport({
        format: exportFormat,
        from: exportFrom ? `${exportFrom}T00:00:00Z` : undefined,
        to: exportTo ? `${exportTo}T23:59:59Z` : undefined,
        type: 'admin',
      });
    } catch {
      setExportError(t('common.toast.downloadFailed', {}));
    } finally {
      setExporting(false);
    }
  };

  return (
    <AdminContentContainer
      title={t('pages.admin.activity.title', {})}
      search={search}
      setSearch={setSearch}
      contentRight={filterUserUuid ? <ClearUserFilterButton onClick={() => setFilterUserUuid(null)} /> : null}
      registry={window.extensionContext.extensionRegistry.pages.admin.activity.container}
    >
      <div className='flex flex-wrap items-end gap-2 mb-4'>
        <label className='flex flex-col gap-1 text-sm'>
          From
          <input
            type='date'
            value={exportFrom}
            onChange={(event) => setExportFrom(event.target.value)}
            className='rounded border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-2 py-1'
          />
        </label>
        <label className='flex flex-col gap-1 text-sm'>
          To
          <input
            type='date'
            value={exportTo}
            onChange={(event) => setExportTo(event.target.value)}
            className='rounded border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-2 py-1'
          />
        </label>
        <label className='flex flex-col gap-1 text-sm'>
          Format
          <select
            value={exportFormat}
            onChange={(event) => setExportFormat(event.target.value as 'csv' | 'json')}
            className='rounded border border-(--mantine-color-default-border) bg-(--mantine-color-body) px-2 py-1'
          >
            <option value='csv'>CSV</option>
            <option value='json'>JSON</option>
          </select>
        </label>
        <Button onClick={exportActivity} loading={exporting}>
          {t('common.button.export', {})}
        </Button>
        {exportError && <span className='text-sm text-(--mantine-color-error)'>{exportError}</span>}
      </div>

      <Table
        columns={activityColumns()}
        loading={loading}
        error={error}
        pagination={activities}
        onPageSelect={setPage}
        onPerPageSelect={setPerPage}
      >
        {activities?.data.map((activity, index) => (
          <ActivityRow
            key={`${activity.created.toISOString()}-${index}`}
            activity={activity}
            showAvatar
            avatar={activity.user}
            linkActor
          />
        ))}
      </Table>
    </AdminContentContainer>
  );
}
