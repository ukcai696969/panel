import { useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import sendConsoleCommand from '@/api/server/sendConsoleCommand.ts';
import Button from '@/elements/buttons/Button.tsx';
import { ServerCan } from '@/elements/Can.tsx';
import Select from '@/elements/input/Select.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { MINECRAFT_ITEM_ID, MINECRAFT_PLAYER_NAME } from '@/lib/players.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';

/**
 * Quick commands for a Minecraft server, sent through the same
 * `POST /command` endpoint as the console input (and therefore the same
 * `control.console` permission and activity logging). Only rendered for
 * users with that permission, via `ServerCan` in the parent and here.
 *
 * Presets: `say`, `give <player> <item> [count]`, `tp <player> <target>`.
 * Player names are validated against the Minecraft username rules and item
 * ids against `namespace:item` characters; the backend command endpoint
 * still applies its own 1024-character limit.
 */
export default function QuickCommandBar({ players }: { players: string[] }) {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const server = useServerStore((state) => state.server);
  const powerState = useServerStore((state) => state.state);

  const [sayMessage, setSayMessage] = useState('');
  const [givePlayer, setGivePlayer] = useState<string | null>(null);
  const [giveItem, setGiveItem] = useState('');
  const [giveCount, setGiveCount] = useState('1');
  const [tpPlayer, setTpPlayer] = useState<string | null>(null);
  const [tpTarget, setTpTarget] = useState<string | null>(null);
  const [sending, setSending] = useState<string | null>(null);

  const disabled = powerState !== 'running';
  const playerOptions = players.map((player) => ({ label: player, value: player }));

  const send = (key: string, command: string) => {
    setSending(key);
    sendConsoleCommand(server.uuid, command)
      .then(() => addToast(t('pages.server.console.players.toast.commandSent', {}), 'success'))
      .catch((msg) => addToast(httpErrorToHuman(msg), 'error'))
      .finally(() => setSending(null));
  };

  const giveCountNumber = Number(giveCount);
  const giveValid =
    !!givePlayer &&
    MINECRAFT_PLAYER_NAME.test(givePlayer) &&
    MINECRAFT_ITEM_ID.test(giveItem.trim()) &&
    Number.isInteger(giveCountNumber) &&
    giveCountNumber >= 1 &&
    giveCountNumber <= 64;

  return (
    <ServerCan action='control.console'>
      <Stack gap='sm'>
        <Text fw={600}>{t('pages.server.console.players.quick.title', {})}</Text>

        <Group gap='xs' align='flex-end'>
          <TextInput
            label={t('pages.server.console.players.quick.message', {})}
            placeholder='Hello, players!'
            value={sayMessage}
            onChange={(e) => setSayMessage(e.currentTarget.value)}
            disabled={disabled}
            className='flex-1 min-w-48'
          />
          <Button
            loading={sending === 'say'}
            disabled={disabled || !sayMessage.trim()}
            onClick={() => send('say', `say ${sayMessage.trim()}`)}
          >
            {t('pages.server.console.players.quick.send', {})}
          </Button>
        </Group>

        <Group gap='xs' align='flex-end'>
          <Select
            label={t('pages.server.console.players.quick.player', {})}
            placeholder={t('pages.server.console.players.quick.player', {})}
            data={playerOptions}
            value={givePlayer}
            onChange={setGivePlayer}
            disabled={disabled || players.length === 0}
            className='flex-1 min-w-32'
          />
          <TextInput
            label={t('pages.server.console.players.quick.item', {})}
            placeholder='minecraft:diamond_sword'
            value={giveItem}
            onChange={(e) => setGiveItem(e.currentTarget.value)}
            disabled={disabled}
            className='flex-1 min-w-40'
          />
          <TextInput
            label={t('pages.server.console.players.quick.count', {})}
            value={giveCount}
            onChange={(e) => setGiveCount(e.currentTarget.value.replace(/[^0-9]/g, ''))}
            disabled={disabled}
            className='w-20'
          />
          <Button
            loading={sending === 'give'}
            disabled={disabled || !giveValid}
            onClick={() =>
              send('give', `give ${givePlayer} ${giveItem.trim()} ${giveCountNumber}`)
            }
          >
            {t('pages.server.console.players.quick.send', {})}
          </Button>
        </Group>

        <Group gap='xs' align='flex-end'>
          <Select
            label={t('pages.server.console.players.quick.player', {})}
            placeholder={t('pages.server.console.players.quick.player', {})}
            data={playerOptions}
            value={tpPlayer}
            onChange={setTpPlayer}
            disabled={disabled || players.length === 0}
            className='flex-1 min-w-32'
          />
          <Select
            label={t('pages.server.console.players.quick.target', {})}
            placeholder={t('pages.server.console.players.quick.target', {})}
            data={playerOptions}
            value={tpTarget}
            onChange={setTpTarget}
            disabled={disabled || players.length === 0}
            className='flex-1 min-w-32'
          />
          <Button
            loading={sending === 'teleport'}
            disabled={disabled || !tpPlayer || !tpTarget}
            onClick={() => send('teleport', `tp ${tpPlayer} ${tpTarget}`)}
          >
            {t('pages.server.console.players.quick.send', {})}
          </Button>
        </Group>
      </Stack>
    </ServerCan>
  );
}
