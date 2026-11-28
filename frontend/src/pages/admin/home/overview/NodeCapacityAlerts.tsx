import { faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import getNodeCapacities from '@/api/admin/nodes/getNodeCapacities.ts';
import getNodes from '@/api/admin/nodes/getNodes.ts';
import Badge from '@/elements/data-display/Badge.tsx';
import NodeCapacityBar from '@/elements/data-display/NodeCapacityBar.tsx';
import TableLink from '@/elements/data-display/TableLink.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import { getNodeDeploymentUsage, getNodeResourceWarnings } from '@/lib/domain/node.ts';
import { queryKeys } from '@/lib/queryKeys.ts';
import { useResource } from '@/plugins/resource/useResource.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

/**
 * Admin home card listing every node whose allocated RAM or disk is at or
 * above 85%, with warning badges and capacity bars. Renders nothing when
 * no node is over the threshold. Data comes from the existing nodes list
 * and `/api/admin/nodes/capacities` endpoints — no new backend needed.
 */
export default function NodeCapacityAlerts() {
  const { t } = useTranslations();

  const { data: capacities } = useResource({
    queryKey: queryKeys.admin.nodes.capacities(),
    queryFn: getNodeCapacities,
    silent: true,
  });

  const { data: nodes } = useResource({
    queryKey: [...queryKeys.admin.nodes.all(), 'capacity-alerts'],
    queryFn: async () => {
      const all = [];
      let page = 1;

      for (;;) {
        const result = await getNodes(page);
        all.push(...result.data);
        if (page * result.perPage >= result.total) break;
        page += 1;
      }

      return all;
    },
    silent: true,
  });

  const warnedNodes = (nodes ?? [])
    .map((node) => {
      const allocated = capacities?.[node.uuid];
      const usage = allocated ? getNodeDeploymentUsage(node, allocated) : null;
      return { node, usage, warnings: getNodeResourceWarnings(usage) };
    })
    .filter(({ warnings }) => warnings.length > 0);

  if (warnedNodes.length === 0) {
    return null;
  }

  return (
    <TitleCard
      title={t('pages.admin.home.tabs.overview.page.card.nodeCapacity', {})}
      icon={faTriangleExclamation}
      className='mt-4'
    >
      <div className='flex flex-col gap-4'>
        {warnedNodes.map(({ node, usage, warnings }) => (
          <div key={node.uuid} className='flex flex-col gap-2'>
            <div className='flex gap-2 items-center flex-wrap'>
              <TableLink to={`/admin/nodes/${node.uuid}`}>{node.name}</TableLink>
              {warnings.map((warning) => (
                <Badge key={warning.resource} color='red' variant='light'>
                  {warning.resource === 'memory'
                    ? t('pages.admin.home.tabs.overview.page.nodeCapacity.over85Memory', {
                        percent: Math.round(warning.ratio * 100),
                      })
                    : t('pages.admin.home.tabs.overview.page.nodeCapacity.over85Disk', {
                        percent: Math.round(warning.ratio * 100),
                      })}
                </Badge>
              ))}
            </div>
            {usage && (
              <div className='grid grid-cols-1 md:grid-cols-2 gap-4'>
                <NodeCapacityBar
                  label={t('common.form.memory', {})}
                  usedMb={usage.memory.used}
                  limitMb={usage.memory.limit}
                />
                <NodeCapacityBar
                  label={t('pages.admin.servers.tabs.general.page.form.diskSpace', {})}
                  usedMb={usage.disk.used}
                  limitMb={usage.disk.limit}
                />
              </div>
            )}
          </div>
        ))}
      </div>
      <div className='flex gap-2 items-center mt-4 text-sm text-(--mantine-color-dimmed)'>
        <FontAwesomeIcon icon={faTriangleExclamation} />
        {t('pages.admin.home.tabs.overview.page.nodeCapacity.hint', {})}
      </div>
    </TitleCard>
  );
}
