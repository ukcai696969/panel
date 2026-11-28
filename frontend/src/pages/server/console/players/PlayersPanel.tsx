import { faArrowRotateRight, faUsers } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import getPlayers from '@/api/server/players/getPlayers.ts';
import sendConsoleCommand from '@/api/server/sendConsoleCommand.ts';
import Button from '@/elements/buttons/Button.tsx';
import { ServerCan } from '@/elements/Can.tsx';
import Card from '@/elements/data-display/Card.tsx';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import EmptyState from '@/elements/feedback/EmptyState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Text from '@/elements/typography/Text.tsx';
import Title from '@/elements/typography/Title.tsx';
import { applyConsoleLine, isMinecraftLikeServer } from '@/lib/players.ts';
import useWebsocketEvent, { SocketEvent } from '@/plugins/websocket/useWebsocketEvent.ts';
import { usePollingResource } from '@/plugins/resource/usePollingResource.ts';
import { useServerCan } from '@/plugins/usePermissions.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import QuickCommandBar from './QuickCommandBar.tsx';

type PlayerAction = 'kick' | 'ban';

/**
 * "Online players" panel for Minecraft / supported eggs, shown under the
 * console on the server console page.
 *
 * The list comes from `GET /players` (best-effort parse of the server's
 * recent console output, see the backend route), polled every 30 seconds.
 * Live console lines are also applied on top (joins/leaves and fresh `list`
 * output) so the panel updates without waiting for the next poll.
 *
 * Refresh sends `list` through the existing `POST /command` endpoint and
 * re-fetches a moment later. Kick/ban/whitelist are the same vanilla
 * commands an operator would type, sent through that endpoint — so they
 * need `control.console`, are activity-logged by the backend, and do nothing
 * the console itself could not do. The panel is hidden entirely for eggs
 * that are not Minecraft-like.
 */
export default function PlayersPanel() {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const server = useServerStore((state) => state.server);
  const powerState = useServerStore((state) => state.state);
  const canReadConsole = useServerCan('control.read-console');
  const canConsole = useServerCan('control.console');

  const [livePlayers, setLivePlayers] = useState<string[] | null>(null);
  const [confirmAction, setConfirmAction] = useState<{ action: PlayerAction; player: string } | null>(null);
  const [reason, setReason] = useState('');
  const [refreshing, setRefreshing] = useState(false);

  const {
    data: playersData,
    loading,
    refetch,
  } = usePollingResource({
    queryKey: ['server', server.uuid, 'players'],
    queryFn: () => getPlayers(server.uuid),
    interval: 30_000,
    enabled: canReadConsole,
    silent: true,
  });

  useEffect(() => {
    if (playersData) setLivePlayers(playersData.players);
  }, [playersData]);

  useWebsocketEvent(SocketEvent.CONSOLE_OUTPUT, (line) => {
    setLivePlayers((prev) => {
      if (!prev) return prev;
      return applyConsoleLine(prev, line ?? '') ?? prev;
    });
  });

  if (!isMinecraftLikeServer(server)) return null;

  const players = livePlayers ?? playersData?.players ?? [];
  const max = playersData?.max ?? null;
  const stale = playersData?.stale ?? true;
  const running = powerState === 'running';

  const refresh = () => {
    if (!canConsole || !running) {
      refetch();
      return;
    }

    setRefreshing(true);
    sendConsoleCommand(server.uuid, 'list')
      .catch((msg) => addToast(httpErrorToHuman(msg), 'error'))
      .finally(() => {
        window.setTimeout(() => {
          refetch();
          setRefreshing(false);
        }, 1500);
      });
  };

  const runPlayerCommand = (command: string, toastKey: string, player: string) => {
    sendConsoleCommand(server.uuid, command)
      .then(() => {
        addToast(t(toastKey, { player }), 'success');
        window.setTimeout(() => refetch(), 1000);
      })
      .catch((msg) => addToast(httpErrorToHuman(msg), 'error'));
  };

  const confirmPlayerAction = () => {
    if (!confirmAction) return;

    const suffix = reason.trim() ? ` ${reason.trim()}` : '';
    const command =
      confirmAction.action === 'kick'
        ? `kick ${confirmAction.player}${suffix}`
        : `ban ${confirmAction.player}${suffix}`;
    runPlayerCommand(
      command,
      confirmAction.action === 'kick'
        ? 'pages.server.console.players.toast.kicked'
        : 'pages.server.console.players.toast.banned',
      confirmAction.player,
    );
    setConfirmAction(null);
    setReason('');
  };

  return (
    <Card p='md'>
      <ConfirmationModal
        opened={confirmAction !== null}
        onClose={() => {
          setConfirmAction(null);
          setReason('');
        }}
        title={
          confirmAction
            ? t(
                confirmAction.action === 'kick'
                  ? 'pages.server.console.players.modal.kick.title'
                  : 'pages.server.console.players.modal.ban.title',
                { player: confirmAction.player },
              )
            : ''
        }
        confirm={t('common.button.send', {})}
        confirmColor='red'
        onConfirmed={confirmPlayerAction}
      >
        <TextInput
          label={t('pages.server.console.players.modal.reason', {})}
          value={reason}
          onChange={(e) => setReason(e.currentTarget.value)}
          maxLength={256}
        />
      </ConfirmationModal>

      <Stack gap='md'>
        <Group justify='space-between' align='center'>
          <div>
            <Title order={3}>{t('pages.server.console.players.title', {})}</Title>
            <Text size='sm' c='dimmed'>
              {max !== null
                ? t('pages.server.console.players.count', { online: players.length, max })
                : t('pages.server.console.players.countUnknownMax', { online: players.length })}
            </Text>
          </div>
          <Tooltip label={t('pages.server.console.players.button.refresh', {})}>
            <ActionIcon
              variant='default'
              aria-label={t('pages.server.console.players.button.refresh', {})}
              disabled={!canReadConsole || !running || refreshing}
              onClick={refresh}
            >
              <FontAwesomeIcon icon={faArrowRotateRight} />
            </ActionIcon>
          </Tooltip>
        </Group>

        {stale && players.length === 0 && (
          <Text size='sm' c='dimmed'>
            {t('pages.server.console.players.stale', {})}
          </Text>
        )}

        {!playersData && loading ? (
          <Spinner.Centered />
        ) : players.length === 0 ? (
          <EmptyState icon={faUsers} title={t('pages.server.console.players.empty', {})} />
        ) : (
          <Stack gap='xs'>
            {players.map((player) => (
              <Group key={player} justify='space-between' wrap='nowrap'>
                <Text className='font-mono truncate'>{player}</Text>
                <ServerCan action='control.console'>
                  <Group gap='xs' wrap='nowrap'>
                    <Button
                      size='xs'
                      variant='subtle'
                      disabled={!running}
                      onClick={() => setConfirmAction({ action: 'kick', player })}
                    >
                      {t('pages.server.console.players.button.kick', {})}
                    </Button>
                    <Button
                      size='xs'
                      variant='subtle'
                      color='red'
                      disabled={!running}
                      onClick={() => setConfirmAction({ action: 'ban', player })}
                    >
                      {t('pages.server.console.players.button.ban', {})}
                    </Button>
                    <Button
                      size='xs'
                      variant='subtle'
                      disabled={!running}
                      onClick={() =>
                        runPlayerCommand(
                          `whitelist add ${player}`,
                          'pages.server.console.players.toast.whitelistAdded',
                          player,
                        )
                      }
                    >
                      {t('pages.server.console.players.button.whitelistAdd', {})}
                    </Button>
                    <Button
                      size='xs'
                      variant='subtle'
                      disabled={!running}
                      onClick={() =>
                        runPlayerCommand(
                          `whitelist remove ${player}`,
                          'pages.server.console.players.toast.whitelistRemoved',
                          player,
                        )
                      }
                    >
                      {t('pages.server.console.players.button.whitelistRemove', {})}
                    </Button>
                  </Group>
                </ServerCan>
              </Group>
            ))}
          </Stack>
        )}

        {canConsole && <QuickCommandBar players={players} />}
      </Stack>
    </Card>
  );
}
