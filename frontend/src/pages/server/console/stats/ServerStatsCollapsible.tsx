import { faChartSimple, faChevronDown } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Collapse } from '@mantine/core';
import classNames from 'classnames';
import { useState } from 'react';
import Card from '@/elements/data-display/Card.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import ServerMetricTiles from './ServerMetricTiles.tsx';

/**
 * Mobile replacement for the always-visible metric tiles column on the
 * console page. On a phone the CPU / memory / disk / network tiles push
 * the terminal off screen, so they live behind a single large (44px)
 * toggle and start collapsed. Desktop (`xl` and up) keeps rendering
 * `ServerMetricTiles` directly in the grid column — see ServerConsole.
 */
export default function ServerStatsCollapsible() {
  const { t } = useTranslations();
  const [expanded, setExpanded] = useState(false);

  return (
    <Card px='md' py='sm' className='mb-4'>
      <button
        type='button'
        aria-expanded={expanded}
        onClick={() => setExpanded((value) => !value)}
        className='flex min-h-11 w-full items-center gap-2 text-left text-sm font-medium'
      >
        <FontAwesomeIcon icon={faChartSimple} className='text-(--mantine-color-dimmed)' />
        <span className='flex-1'>{t('common.stat.resources', {})}</span>
        <FontAwesomeIcon
          icon={faChevronDown}
          className={classNames('text-(--mantine-color-dimmed) transition-transform', expanded && 'rotate-180')}
        />
      </button>
      <Collapse in={expanded}>
        <div className='pt-3'>
          <ServerMetricTiles />
        </div>
      </Collapse>
    </Card>
  );
}
