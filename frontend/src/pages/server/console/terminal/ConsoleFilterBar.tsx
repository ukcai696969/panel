import { faDownload } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Switch from '@/elements/input/Switch.tsx';
import Group from '@/elements/layout/Group.tsx';
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import Text from '@/elements/typography/Text.tsx';
import { isErrorConsoleLine } from '@/lib/players.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

export interface ConsoleFilter {
  text: string;
  errorsOnly: boolean;
}

interface ConsoleFilterBarProps {
  filter: ConsoleFilter;
  setFilter: (filter: ConsoleFilter) => void;
  /** Matching lines (at most the newest 200), or `null` when no filter is active. */
  filteredLines: string[] | null;
  /** Total matches before the 200-line cap, or `null` when no filter is active. */
  matchCount: number | null;
  totalLines: number;
  canDownload: boolean;
  downloading: boolean;
  onDownload: () => void;
}

/**
 * Console search/filter bar, rendered above the terminal.
 *
 * This is a *filter*, not the xterm find-next search that already lives in
 * the terminal header (Ctrl+F): it lists the matching console lines in a
 * scrollable panel, with error lines highlighted red, and offers a
 * "Download console log" button that fetches the full log from the backend.
 * The live xterm terminal is left untouched — filtering never hides or
 * clears it — so this cannot lose console output.
 */
export default function ConsoleFilterBar({
  filter,
  setFilter,
  filteredLines,
  matchCount,
  totalLines,
  canDownload,
  downloading,
  onDownload,
}: ConsoleFilterBarProps) {
  const { t } = useTranslations();
  const active = filteredLines !== null;

  return (
    <div className='flex flex-col gap-2 mb-2'>
      <Group gap='xs' align='center' wrap='wrap'>
        <TextInput
          placeholder={t('pages.server.console.filter.placeholder', {})}
          aria-label={t('pages.server.console.filter.placeholder', {})}
          value={filter.text}
          onChange={(e) => setFilter({ ...filter, text: e.currentTarget.value })}
          className='flex-1 min-w-40'
        />
        <Switch
          label={t('pages.server.console.filter.errorsOnly', {})}
          checked={filter.errorsOnly}
          onChange={(e) => setFilter({ ...filter, errorsOnly: e.currentTarget.checked })}
        />
        <Tooltip label={t('pages.server.console.filter.download', {})}>
          <ActionIcon
            variant='default'
            aria-label={t('pages.server.console.filter.download', {})}
            disabled={!canDownload || downloading}
            onClick={onDownload}
          >
            <FontAwesomeIcon icon={faDownload} />
          </ActionIcon>
        </Tooltip>
        {active && matchCount !== null && (
          <Text size='xs' c='dimmed'>
            {t('pages.server.console.filter.matches', { count: matchCount, total: totalLines })}
          </Text>
        )}
      </Group>

      {active && filteredLines && (
        <div className='max-h-64 overflow-y-auto rounded-sm border border-(--mantine-color-default-border) bg-(--mantine-color-default) p-2 font-mono text-xs whitespace-pre-wrap break-all'>
          {filteredLines.length === 0 ? (
            <Text size='xs' c='dimmed'>
              {t('pages.server.console.filter.noMatches', {})}
            </Text>
          ) : (
            filteredLines.map((line, index) => (
              <div key={`${index}-${line.slice(0, 32)}`} className={isErrorConsoleLine(line) ? 'text-red-500' : undefined}>
                {line}
              </div>
            ))
          )}
          {matchCount !== null && matchCount > filteredLines.length && (
            <Text size='xs' c='dimmed'>
              {t('pages.server.console.filter.showingLatest', { count: filteredLines.length })}
            </Text>
          )}
        </div>
      )}
    </div>
  );
}
