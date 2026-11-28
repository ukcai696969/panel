import { faHeart, faHeartBroken } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { forwardRef, ReactNode } from 'react';
import { z } from 'zod';
import Badge from '@/elements/data-display/Badge.tsx';
import NodeCapacityBar from '@/elements/data-display/NodeCapacityBar.tsx';
import { TableData, TableRow, TableSelectionCell } from '@/elements/data-display/Table.tsx';
import TableLink from '@/elements/data-display/TableLink.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import { ContextMenuChildrenProps, ContextMenuToggle } from '@/elements/overlays/ContextMenu.tsx';
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import FormattedTimestamp from '@/elements/time/FormattedTimestamp.tsx';
import Code from '@/elements/typography/Code.tsx';
import { getNodeResourceWarnings, isNodeAIO, nodeDeploymentStateInfo } from '@/lib/domain/node.ts';
import { bytesToString, mbToBytes } from '@/lib/format/size.ts';
import { AdminNode } from '@/lib/schemas/admin/nodes.ts';
import { adminUpgradeStatusSchema } from '@/lib/schemas/admin/system.ts';
import { useNodeDeployment } from '@/plugins/nodes/useNodeDeployment.ts';
import { useNodeVersion } from '@/plugins/nodes/useNodeVersion.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

interface NodeRowProps {
  node: AdminNode;
  upgradeStatus?: z.infer<typeof adminUpgradeStatusSchema>;
  desync?: number;
  isSelected?: boolean;
  onSelectionChange?: (selected: boolean, range?: boolean) => void;
  contextMenuProps?: ContextMenuChildrenProps;
  nameSection?: ReactNode;
}

const NodeRow = forwardRef<HTMLTableRowElement, NodeRowProps>(function NodeRow(
  { node, upgradeStatus, desync, isSelected = false, onSelectionChange, contextMenuProps, nameSection },
  ref,
) {
  const { t } = useTranslations();
  const { version, unavailable, loading, upgradeAvailability } = useNodeVersion(node, { upgradeStatus });
  const {
    state: deploymentState,
    usage: deploymentUsage,
    servers: deploymentServers,
    loading: deploymentLoading,
  } = useNodeDeployment(node);
  const resourceWarnings = getNodeResourceWarnings(deploymentUsage);

  return (
    <TableRow
      bg={isSelected ? 'var(--mantine-color-blue-light)' : undefined}
      onClick={(e) => {
        if (e.ctrlKey || e.metaKey || e.shiftKey) {
          onSelectionChange?.(true, e.shiftKey);
          return true;
        }
        return false;
      }}
      onContextMenu={(e) => {
        if (!contextMenuProps) return;

        e.preventDefault();
        contextMenuProps.openMenu(e.clientX, e.clientY);
      }}
      ref={ref}
    >
      {onSelectionChange !== undefined && (
        <TableSelectionCell id={node.uuid} checked={isSelected} onChange={onSelectionChange} />
      )}

      <TableData>
        {loading ? (
          <Spinner size={16} />
        ) : unavailable || !version ? (
          <Tooltip label={t('pages.admin.nodes.tabs.general.page.tooltip.errorWhileFetchingVersion', {})}>
            <FontAwesomeIcon icon={faHeartBroken} className='text-(--mantine-color-red-text)' />
          </Tooltip>
        ) : upgradeAvailability === 'available' ? (
          <Tooltip label={t('pages.admin.nodes.tabs.general.page.tooltip.updateAvailable', { version })}>
            <FontAwesomeIcon icon={faHeart} className='text-(--mantine-color-yellow-text) animate-pulse' />
          </Tooltip>
        ) : upgradeAvailability === 'unknown' ? (
          <Tooltip label={t('pages.admin.nodes.tabs.general.page.tooltip.latestVersionUnknown', { version })}>
            <FontAwesomeIcon icon={faHeart} className='text-(--mantine-color-dimmed) animate-pulse' />
          </Tooltip>
        ) : (
          <Tooltip label={version}>
            <FontAwesomeIcon icon={faHeart} className='text-(--mantine-color-green-text) animate-pulse' />
          </Tooltip>
        )}
      </TableData>

      <TableData>
        <TableLink to={`/admin/nodes/${node.uuid}`}>
          <Code>{node.uuid}</Code>
        </TableLink>
      </TableData>

      {desync !== undefined && <TableData>{desync}ms</TableData>}

      <TableData>
        <span className='flex gap-2 items-center'>
          {node.name}&nbsp;
          <Tooltip
            label={
              deploymentUsage ? (
                <>
                  <div>
                    {deploymentUsage.memory.limit === 0
                      ? t('common.node.deployment.memoryUsageUnlimited', {
                          used: bytesToString(mbToBytes(deploymentUsage.memory.used)),
                        })
                      : t('common.node.deployment.memoryUsage', {
                          used: bytesToString(mbToBytes(deploymentUsage.memory.used)),
                          limit: bytesToString(mbToBytes(deploymentUsage.memory.limit)),
                        })}
                  </div>
                  <div>
                    {deploymentUsage.disk.limit === 0
                      ? t('common.node.deployment.diskUsageUnlimited', {
                          used: bytesToString(mbToBytes(deploymentUsage.disk.used)),
                        })
                      : t('common.node.deployment.diskUsage', {
                          used: bytesToString(mbToBytes(deploymentUsage.disk.used)),
                          limit: bytesToString(mbToBytes(deploymentUsage.disk.limit)),
                        })}
                  </div>
                </>
              ) : (
                nodeDeploymentStateInfo[deploymentState].label()
              )
            }
          >
            <Badge
              color={nodeDeploymentStateInfo[deploymentState].badgeColor}
              variant='light'
              loading={deploymentLoading}
            >
              {nodeDeploymentStateInfo[deploymentState].label()}
            </Badge>
          </Tooltip>
          {isNodeAIO(node) && (
            <Tooltip label={t('pages.admin.nodes.tabs.general.page.tooltip.allInOneNode', {})}>
              <FontAwesomeIcon icon={faHeart} className='text-(--mantine-color-violet-text)' />
            </Tooltip>
          )}
          {nameSection}
        </span>
        {resourceWarnings.length > 0 && (
          <span className='flex gap-2 items-center mt-1 flex-wrap'>
            {resourceWarnings.map((warning) => (
              <Badge key={warning.resource} color='red' variant='light'>
                {warning.resource === 'memory'
                  ? t('common.node.deployment.over85Memory', { percent: Math.round(warning.ratio * 100) })
                  : t('common.node.deployment.over85Disk', { percent: Math.round(warning.ratio * 100) })}
              </Badge>
            ))}
          </span>
        )}
        {deploymentUsage && resourceWarnings.length > 0 && (
          <div className='mt-2 flex flex-col gap-1 min-w-48 max-w-72'>
            <NodeCapacityBar
              label={t('common.form.memory', {})}
              usedMb={deploymentUsage.memory.used}
              limitMb={deploymentUsage.memory.limit}
            />
            <NodeCapacityBar
              label={t('pages.admin.servers.tabs.general.page.form.diskSpace', {})}
              usedMb={deploymentUsage.disk.used}
              limitMb={deploymentUsage.disk.limit}
            />
          </div>
        )}
      </TableData>

      <TableData>
        <TableLink to={`/admin/locations/${node.location.uuid}`} className='block w-fit'>
          <Code className='flex flex-row items-center w-fit'>
            {node.location.flag && (
              <img
                src={`/flags/${node.location.flag}.svg`}
                alt={node.location.name}
                className='w-5 h-5 mr-1 rounded-md shrink-0 my-auto'
              />
            )}{' '}
            {node.location.name}
          </Code>
        </TableLink>
      </TableData>

      <TableData>{deploymentServers}</TableData>

      <TableData>
        <FormattedTimestamp timestamp={node.created} />
      </TableData>

      {contextMenuProps && (
        <TableData className='relative cursor-pointer min-w-10 text-center'>
          <ContextMenuToggle items={contextMenuProps.items} openMenu={contextMenuProps.openMenu} />
        </TableData>
      )}
    </TableRow>
  );
});

export default NodeRow;
