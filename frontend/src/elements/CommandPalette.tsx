import { faMagnifyingGlass, faServer, faUser } from '@fortawesome/free-solid-svg-icons';
import { faComputer } from '@fortawesome/free-solid-svg-icons/faComputer';
import { faEgg } from '@fortawesome/free-solid-svg-icons/faEgg';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { Combobox, useCombobox } from '@mantine/core';
import { ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router';
import { z } from 'zod';
import getAdminNodes from '@/api/admin/nodes/getNodes.ts';
import getAdminServers from '@/api/admin/servers/getServers.ts';
import getAdminUsers from '@/api/admin/users/getUsers.ts';
import searchEggs, { adminEggWithNestSchema } from '@/api/admin/eggs/searchEggs.ts';
import getClientServers from '@/api/server/getServers.ts';
import Avatar from '@/elements/data-display/Avatar.tsx';
import Icon, { type IconLike } from '@/elements/data-display/Icon.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Group from '@/elements/layout/Group.tsx';
import { Modal } from '@/elements/modals/Modal.tsx';
import Kbd from '@/elements/typography/Kbd.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useServerQuickActionTarget } from '@/lib/quickActions/coreQuickActions.tsx';
import { queryKeys } from '@/lib/queryKeys.ts';
import { adminNodeSchema } from '@/lib/schemas/admin/nodes.ts';
import { AdminServer } from '@/lib/schemas/admin/servers.ts';
import { adminFullUserSchema } from '@/lib/schemas/admin/users.ts';
import { serverSchema } from '@/lib/schemas/server/server.ts';
import { matchesActiveShortcut } from '@/plugins/quick-actions/useKeyboardShortcuts.ts';
import { useInfiniteSearchableResource } from '@/plugins/resource/useInfiniteSearchableResource.ts';
import { useServerListShowOthers } from '@/plugins/server/useServerListShowOthers.ts';
import { checkPermissions } from '@/plugins/usePermissions.ts';
import { useAuth } from '@/providers/AuthProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { useCommandPaletteStore } from '@/stores/commandPalette.ts';

const MAX_RESULTS_PER_GROUP = 6;
const QUERY_DEBOUNCE_MS = 150;

type PaletteItem = {
  key: string;
  label: string;
  description?: string;
  icon: IconLike;
  content?: ReactNode;
  onSelect: () => void;
};

type PaletteGroup = {
  id: string;
  label: string;
  icon: IconLike;
  items: PaletteItem[];
};

export default function CommandPalette() {
  const { user } = useAuth();
  const setOpen = useCommandPaletteStore((state) => state.setOpen);

  useEffect(() => {
    if (!user) setOpen(false);
  }, [user]);

  if (!user) return null;

  return <Palette />;
}

function Palette() {
  'use no memo';

  const { t } = useTranslations();
  const navigate = useNavigate();
  const { user } = useAuth();
  const [showOthers] = useServerListShowOthers();

  const open = useCommandPaletteStore((state) => state.open);
  const setOpen = useCommandPaletteStore((state) => state.setOpen);

  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');

  // Debounced search: the query is only sent to the list APIs once the user
  // pauses typing, on top of the 150ms debounce inside
  // `useInfiniteSearchableResource`.
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(query), QUERY_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const hasQuery = debouncedQuery.trim().length > 0;

  // Admin list endpoints 403 without the matching permission, so each admin
  // group is only searched when the user can actually read that resource.
  // `user.admin` covers full admins; role grants (including wildcards such as
  // `servers.*`) are checked with `checkPermissions`.
  const canAdmin = useCallback(
    (permission: string) =>
      !!user?.admin || checkPermissions(user?.role?.adminPermissions ?? [], permission).some(Boolean),
    [user],
  );
  const canSearchAdminServers = canAdmin('servers.read');
  const canSearchUsers = canAdmin('users.read');
  const canSearchNodes = canAdmin('nodes.read');
  const canSearchEggs = canAdmin('eggs.read');

  // Ctrl+K / Cmd+K opens the palette from anywhere. This is a plain window
  // listener (not `useKeyboardShortcuts`) on purpose: `files.search` already
  // claims Ctrl+K in the file manager through the shortcut registry, and
  // `matchesActiveShortcut` lets that binding win while it is mounted instead
  // of opening two palettes at once. The shortcut is deliberately not added
  // as a `general.globalSearch` core definition yet because that needs new
  // translation keys (see MANIFEST.md); once they exist, switch this to the
  // id-based `useKeyboardShortcut` form so the binding becomes rebindable in
  // the shortcuts settings. Only the modifier+K chord triggers this — plain
  // typing in inputs is never hijacked.
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== 'k') return;
      if (!event.ctrlKey && !event.metaKey) return;
      if (event.altKey || event.shiftKey) return;
      if (matchesActiveShortcut(event)) return;

      event.preventDefault();
      setOpen(true);
    };

    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [setOpen]);

  const combobox = useCombobox();
  const comboboxRef = useRef(combobox);

  useEffect(() => {
    comboboxRef.current = combobox;
  });

  useEffect(() => {
    if (open) {
      comboboxRef.current.openDropdown();
    } else {
      setQuery('');
    }
  }, [open]);

  const clientServers = useInfiniteSearchableResource<z.infer<typeof serverSchema>>({
    queryKey: [...queryKeys.user.servers.all(), { showOthers }, 'command-palette'],
    fetcher: (page, search) => getClientServers(page, search, showOthers),
    canRequest: open && !canSearchAdminServers && hasQuery,
  });

  const adminServers = useInfiniteSearchableResource<AdminServer>({
    queryKey: [...queryKeys.admin.servers.all(), 'command-palette'],
    fetcher: (page, search) => getAdminServers(page, search, MAX_RESULTS_PER_GROUP),
    canRequest: open && canSearchAdminServers && hasQuery,
  });

  const users = useInfiniteSearchableResource<z.infer<typeof adminFullUserSchema>>({
    queryKey: [...queryKeys.admin.users.all(), 'command-palette'],
    fetcher: (page, search) => getAdminUsers(page, search, MAX_RESULTS_PER_GROUP),
    canRequest: open && canSearchUsers && hasQuery,
  });

  const nodes = useInfiniteSearchableResource<z.infer<typeof adminNodeSchema>>({
    queryKey: [...queryKeys.admin.nodes.all(), 'command-palette'],
    fetcher: (page, search) => getAdminNodes(page, search, MAX_RESULTS_PER_GROUP),
    canRequest: open && canSearchNodes && hasQuery,
  });

  const eggs = useInfiniteSearchableResource<z.infer<typeof adminEggWithNestSchema>>({
    queryKey: [...queryKeys.admin.eggs.all(), 'command-palette'],
    fetcher: (page, search) => searchEggs(null, page, search),
    canRequest: open && canSearchEggs && hasQuery,
  });

  useEffect(() => {
    clientServers.setSearch(debouncedQuery);
    adminServers.setSearch(debouncedQuery);
    users.setSearch(debouncedQuery);
    nodes.setSearch(debouncedQuery);
    eggs.setSearch(debouncedQuery);
  }, [debouncedQuery]);

  const close = () => setOpen(false);
  const serverTarget = useServerQuickActionTarget();

  const go = useCallback(
    (path: string) => {
      close();
      navigate(path);
    },
    [navigate],
  );

  const groups: PaletteGroup[] = useMemo(() => {
    if (!hasQuery) return [];

    const result: PaletteGroup[] = [];

    const serverItems: PaletteItem[] = canSearchAdminServers
      ? adminServers.items.slice(0, MAX_RESULTS_PER_GROUP).map((server) => ({
          key: `admin-server:${server.uuid}`,
          label: server.name,
          description: server.node.name,
          icon: faComputer,
          content: (
            <Group gap={6} wrap='nowrap'>
              <Avatar size={16} src={server.owner.avatar} name={server.owner.username} />
              <Text size='xs' c='inherit' opacity={0.6}>
                {server.owner.username}
              </Text>
            </Group>
          ),
          onSelect: () => go(`/admin/servers/${server.uuid}`),
        }))
      : clientServers.items.slice(0, MAX_RESULTS_PER_GROUP).map((server) => ({
          key: `server:${server.uuid}`,
          label: server.name,
          description: server.nodeName,
          icon: faServer,
          onSelect: () => go(serverTarget(server)),
        }));

    if (serverItems.length > 0) {
      result.push({
        id: 'servers',
        label: t('pages.admin.servers.title', {}),
        icon: faServer,
        items: serverItems,
      });
    }

    const userItems: PaletteItem[] = users.items.slice(0, MAX_RESULTS_PER_GROUP).map((user) => {
      const fullName = [user.nameFirst, user.nameLast].filter(Boolean).join(' ');

      return {
        key: `user:${user.uuid}`,
        label: user.username,
        description: user.email,
        icon: faUser,
        content: fullName ? (
          <Group gap={6} wrap='nowrap'>
            <Avatar size={16} src={user.avatar} name={user.username} />
            <Text size='xs' c='inherit' opacity={0.6}>
              {fullName}
            </Text>
          </Group>
        ) : undefined,
        onSelect: () => go(`/admin/users/${user.uuid}`),
      };
    });

    if (userItems.length > 0) {
      result.push({
        id: 'users',
        label: t('pages.admin.users.title', {}),
        icon: faUser,
        items: userItems,
      });
    }

    const nodeItems: PaletteItem[] = nodes.items.slice(0, MAX_RESULTS_PER_GROUP).map((node) => ({
      key: `node:${node.uuid}`,
      label: node.name,
      description: node.location.name,
      icon: faServer,
      onSelect: () => go(`/admin/nodes/${node.uuid}`),
    }));

    if (nodeItems.length > 0) {
      result.push({
        id: 'nodes',
        label: t('pages.admin.nodes.title', {}),
        icon: faServer,
        items: nodeItems,
      });
    }

    const eggItems: PaletteItem[] = eggs.items.slice(0, MAX_RESULTS_PER_GROUP).map(({ egg, nest }) => ({
      key: `egg:${egg.uuid}`,
      label: egg.name,
      description: nest.name,
      icon: faEgg,
      content: (
        <Text size='xs' c='inherit' opacity={0.6}>
          {egg.author}
        </Text>
      ),
      onSelect: () => go(`/admin/nests/${nest.uuid}/eggs/${egg.uuid}`),
    }));

    if (eggItems.length > 0) {
      result.push({
        id: 'eggs',
        label: t('pages.admin.nests.tabs.eggs.page.title', {}),
        icon: faEgg,
        items: eggItems,
      });
    }

    return result;
  }, [
    hasQuery,
    canSearchAdminServers,
    adminServers.items,
    clientServers.items,
    users.items,
    nodes.items,
    eggs.items,
    serverTarget,
    go,
    t,
  ]);

  const flatItems = useMemo(() => groups.flatMap((group) => group.items), [groups]);
  const flatItemIndex = useMemo(() => new Map(flatItems.map((item, index) => [item.key, index])), [flatItems]);
  const itemsKey = useMemo(() => flatItems.map((item) => item.key).join('|'), [flatItems]);

  useEffect(() => {
    if (open) comboboxRef.current.selectFirstOption();
  }, [open, itemsKey]);

  const loading =
    clientServers.loading || adminServers.loading || users.loading || nodes.loading || eggs.loading;

  const handleOptionSubmit = (value: string) => {
    const item = flatItems.find((item) => item.key === value);
    if (!item) return;

    item.onSelect();
  };

  return (
    <Modal
      opened={open}
      onClose={close}
      withCloseButton={false}
      padding={0}
      size='lg'
      transitionProps={{
        transition: 'pop',
        duration: 150,
        onEntered: () => combobox.selectFirstOption(),
      }}
    >
      <Combobox store={combobox} onOptionSubmit={handleOptionSubmit} size='lg' withinPortal={false}>
        <div className='p-3 border-b border-(--mantine-color-default-border)'>
          <Combobox.Search
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') close();
            }}
            placeholder={t('common.input.search', {})}
            leftSection={<FontAwesomeIcon icon={faMagnifyingGlass} size='sm' />}
            rightSection={hasQuery && loading ? <Spinner size={14} /> : undefined}
            data-autofocus
            styles={{
              input: {
                margin: 0,
                width: '100%',
                border: 'calc(0.0625rem * var(--mantine-scale)) solid var(--input-bd)',
                backgroundColor: 'var(--input-bg)',
                borderRadius: 'var(--input-radius)',
              },
            }}
          />
        </div>

        <div className='max-h-120 overflow-y-auto p-2'>
          <Combobox.Options>
            {!hasQuery && <Combobox.Empty>{t('elements.quickActions.placeholder', {})}</Combobox.Empty>}
            {hasQuery && flatItems.length === 0 && !loading && (
              <Combobox.Empty>{t('elements.selectInput.noResults', {})}</Combobox.Empty>
            )}

            {groups.map((group) => (
              <Combobox.Group
                key={group.id}
                label={
                  <Group gap={6} wrap='nowrap'>
                    <Icon icon={group.icon} />
                    <span>{group.label}</span>
                  </Group>
                }
              >
                {group.items.map((item) => {
                  const index = flatItemIndex.get(item.key)!;
                  const isSelected = index === combobox.selectedOptionIndex;

                  return (
                    <Combobox.Option
                      key={item.key}
                      value={item.key}
                      selected={isSelected}
                      onMouseMove={() => combobox.selectOption(index)}
                    >
                      <Group gap='sm' wrap='nowrap'>
                        <Icon icon={item.icon} />
                        <div className='flex flex-col min-w-0'>
                          <Text size='sm' c='inherit'>
                            {item.label}
                          </Text>
                          {item.content}
                        </div>
                        {item.description && (
                          <Text size='xs' c='inherit' opacity={0.6} ml='auto'>
                            {item.description}
                          </Text>
                        )}
                      </Group>
                    </Combobox.Option>
                  );
                })}
              </Combobox.Group>
            ))}
          </Combobox.Options>
        </div>

        <div className='flex flex-wrap items-center gap-x-5 gap-y-2 border-t border-(--mantine-color-default-border) px-4 py-3'>
          <Group gap={4}>
            <Kbd size='xs'>↑</Kbd>
            <Kbd size='xs'>↓</Kbd>
            <Text size='xs' c='dimmed'>
              {t('elements.quickActions.hint.navigate', {})}
            </Text>
          </Group>
          <Group gap={4}>
            <Kbd size='xs'>Enter</Kbd>
            <Text size='xs' c='dimmed'>
              {t('elements.quickActions.hint.select', {})}
            </Text>
          </Group>
          <Group gap={4}>
            <Kbd size='xs'>Esc</Kbd>
            <Text size='xs' c='dimmed'>
              {t('elements.quickActions.hint.close', {})}
            </Text>
          </Group>
        </div>
      </Combobox>
    </Modal>
  );
}
