import { faMagnifyingGlass } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import Button from '@/elements/buttons/Button.tsx';
import Kbd from '@/elements/typography/Kbd.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useCommandPaletteStore } from '@/stores/commandPalette.ts';

export default function CommandPaletteTrigger() {
  const { t } = useTranslations();
  const setOpen = useCommandPaletteStore((state) => state.setOpen);

  const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0;

  return (
    <Button
      variant='default'
      fullWidth
      className='mt-2'
      leftSection={<FontAwesomeIcon icon={faMagnifyingGlass} size='sm' />}
      rightSection={<Kbd size='xs'>{isMac ? 'Cmd+K' : 'Ctrl+K'}</Kbd>}
      onClick={() => setOpen(true)}
      styles={{ label: { width: '100%', color: 'var(--mantine-color-dimmed)' } }}
    >
      {t('common.input.search', {})}
    </Button>
  );
}
