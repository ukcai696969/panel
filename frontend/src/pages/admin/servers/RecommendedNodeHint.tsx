import { useMemo } from 'react';
import getRecommendedNode from '@/api/admin/nodes/getRecommendedNode.ts';
import Alert from '@/elements/feedback/Alert.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import { useResource } from '@/plugins/resource/useResource.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

interface RecommendedNodeHintProps {
  memory: number;
  cpu: number;
  disk: number;
  locationUuids: string[];
  allowOverallocation: boolean;
  /** Only fetch (and render) when the smart pick is switched on. */
  enabled: boolean;
}

/**
 * Shows which node the smart picker would choose for the server being
 * created, with its reason (e.g. "Most free RAM: 12.4 GiB free"), or a
 * warning when no node in the selected locations can take the server.
 */
export default function RecommendedNodeHint({
  memory,
  cpu,
  disk,
  locationUuids,
  allowOverallocation,
  enabled,
}: RecommendedNodeHintProps) {
  const { t } = useTranslations();

  const queryKey = useMemo(
    () =>
      [
        'admin',
        'nodes',
        'recommended',
        memory,
        cpu,
        disk,
        locationUuids.join(','),
        allowOverallocation,
      ] as const,
    [memory, cpu, disk, locationUuids, allowOverallocation],
  );

  const { data, loading } = useResource({
    queryKey,
    queryFn: () => getRecommendedNode({ memory, cpu, disk, locationUuids, allowOverallocation }),
    enabled: enabled && locationUuids.length > 0,
    silent: true,
  });

  if (!enabled || locationUuids.length === 0) {
    return null;
  }

  if (loading) {
    return <Spinner.Centered />;
  }

  if (!data) {
    return null;
  }

  if (!data.node) {
    return (
      <Alert>
        {t('pages.admin.servers.tabs.general.page.alert.noRecommendedNode', {
          reason: data.reason ?? '',
        })}
      </Alert>
    );
  }

  return (
    <Alert>
      {t('pages.admin.servers.tabs.general.page.alert.recommendedNode', {
        node: data.node.node.name,
        reason: data.node.reason,
      })}
    </Alert>
  );
}
