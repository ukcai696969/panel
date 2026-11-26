import { faDownload, faPuzzlePiece, faSearch } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import getInstalledAddons from '@/api/server/addons/getInstalledAddons.ts';
import getAddonVersions from '@/api/server/addons/getAddonVersions.ts';
import installAddon from '@/api/server/addons/installAddon.ts';
import searchAddons, { type AddonProjectType, type AddonSource } from '@/api/server/addons/searchAddons.ts';
import Button from '@/elements/buttons/Button.tsx';
import { ServerCan } from '@/elements/Can.tsx';
import ServerContentContainer from '@/elements/containers/ServerContentContainer.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import EmptyState from '@/elements/feedback/EmptyState.tsx';
import ErrorState from '@/elements/feedback/ErrorState.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Select from '@/elements/input/Select.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Group from '@/elements/layout/Group.tsx';
import Text from '@/elements/typography/Text.tsx';
import Title from '@/elements/typography/Title.tsx';
import type { z } from 'zod';
import type { addonProjectSchema, addonSearchResultSchema, addonVersionSchema } from '@/lib/schemas/server/addons.ts';
import { useServerCan } from '@/plugins/usePermissions.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useTranslations } from '@/providers/TranslationProvider.tsx';
import { trackActivity } from '@/stores/activity.ts';
import { useServerStore } from '@/stores/server.ts';

type AddonProject = z.infer<typeof addonProjectSchema>;
type AddonVersion = z.infer<typeof addonVersionSchema>;

const PROJECT_TYPES: { value: AddonProjectType | ''; label: string }[] = [
  { value: '', label: 'All types' },
  { value: 'mod', label: 'Mods' },
  { value: 'modpack', label: 'Modpacks' },
  { value: 'plugin', label: 'Plugins' },
  { value: 'datapack', label: 'Datapacks' },
  { value: 'shader', label: 'Shaders' },
  { value: 'resourcepack', label: 'Resource packs' },
];

const DESTINATION_BY_TYPE: Record<string, string> = {
  mod: '/mods',
  plugin: '/plugins',
  datapack: '/world/datapacks',
  shader: '/shaderpacks',
  resourcepack: '/resourcepacks',
  modpack: '/',
};

export default function ServerAddons() {
  const { t } = useTranslations();
  const { addToast } = useToast();
  const server = useServerStore((state) => state.server);
  const canInstall = useServerCan('files.create');

  const [query, setQuery] = useState('');
  const [source, setSource] = useState<AddonSource>('modrinth');
  const [projectType, setProjectType] = useState<AddonProjectType | ''>('');
  const [results, setResults] = useState<z.infer<typeof addonSearchResultSchema> | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<unknown>(null);

  const [selectedProject, setSelectedProject] = useState<AddonProject | null>(null);
  const [versions, setVersions] = useState<AddonVersion[]>([]);
  const [versionsLoading, setVersionsLoading] = useState(false);
  const [versionsError, setVersionsError] = useState<unknown>(null);
  const [selectedVersionId, setSelectedVersionId] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  const [installed, setInstalled] = useState<Awaited<ReturnType<typeof getInstalledAddons>> | null>(null);
  const [installedLoading, setInstalledLoading] = useState(true);

  const loadInstalled = () => {
    setInstalledLoading(true);
    trackActivity(getInstalledAddons(server.uuid))
      .then(setInstalled)
      .catch(() => setInstalled(null))
      .finally(() => setInstalledLoading(false));
  };

  useEffect(loadInstalled, [server.uuid]);

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults(null);
      setSearchError(null);
      return;
    }

    setSearching(true);
    const handle = setTimeout(() => {
      trackActivity(searchAddons(server.uuid, query.trim(), { projectType, source }))
        .then((data) => {
          setResults(data);
          setSearchError(null);
        })
        .catch(setSearchError)
        .finally(() => setSearching(false));
    }, 500);

    return () => clearTimeout(handle);
  }, [query, projectType, source, server.uuid]);

  const openVersions = (project: AddonProject) => {
    setSelectedProject(project);
    setVersions([]);
    setSelectedVersionId(null);
    setVersionsError(null);
    setVersionsLoading(true);

    trackActivity(getAddonVersions(server.uuid, project.id, project.source))
      .then((data) => {
        setVersions(data.versions);
        const firstCompatible = data.versions.find((version) => version.compatible);
        setSelectedVersionId(firstCompatible?.id ?? data.versions[0]?.id ?? null);
      })
      .catch(setVersionsError)
      .finally(() => setVersionsLoading(false));
  };

  const doInstall = () => {
    if (!selectedProject || !selectedVersionId) return;

    const version = versions.find((item) => item.id === selectedVersionId);
    if (!version) return;

    setInstalling(true);
    trackActivity(
      installAddon(server.uuid, {
        source: selectedProject.source,
        projectId: selectedProject.id,
        versionId: version.id,
        projectType: selectedProject.projectType,
      }),
    )
      .then(() => {
        addToast(`${selectedProject.title} is being installed.`, 'success');
        setSelectedProject(null);
        loadInstalled();
      })
      .catch((error) => addToast(httpErrorToHuman(error), 'error'))
      .finally(() => setInstalling(false));
  };

  const selectedVersion = versions.find((version) => version.id === selectedVersionId) ?? null;
  const context = results?.context;

  return (
    <ServerContentContainer
      title='Add-ons'
      subtitle='Browse Modrinth and install mods, plugins and modpacks in one click.'
    >
      <Group mb='md'>
        <TextInput
          className='flex-1'
          placeholder={t('common.input.search', {})}
          leftSection={<FontAwesomeIcon icon={faSearch} />}
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
        />
        <Select
          className='w-40'
          data={PROJECT_TYPES}
          value={projectType}
          onChange={(value) => setProjectType((value ?? '') as AddonProjectType | '')}
        />
        <Select
          className='w-40'
          data={[
            { value: 'modrinth', label: 'Modrinth' },
            { value: 'curseforge', label: 'CurseForge' },
          ]}
          value={source}
          onChange={(value) => setSource((value ?? 'modrinth') as AddonSource)}
        />
      </Group>

      {source === 'curseforge' && (
        <Text c='dimmed' size='sm' mb='md'>
          CurseForge is a second source and only works when the host has set a CurseForge API key on the panel. If it
          is not configured, searches here will fail — use Modrinth instead.
        </Text>
      )}

      {context && !context.gameVersion && (
        <Text c='dimmed' size='sm' mb='md'>
          This server&apos;s Minecraft version could not be detected from its startup variables, so versions are shown
          unfiltered. Set a MINECRAFT_VERSION variable on the Startup page to filter by version.
        </Text>
      )}
      {context?.gameVersion && (
        <Text c='dimmed' size='sm' mb='md'>
          Filtering for Minecraft {context.gameVersion}
          {context.loaders.length > 0 ? ` (${context.loaders.join(', ')})` : ''}.
        </Text>
      )}

      {searching && <Spinner.Centered />}
      {searchError && <ErrorState error={httpErrorToHuman(searchError)} />}

      {!searching && !searchError && query.trim().length >= 2 && results?.projects.length === 0 && (
        <EmptyState
          icon={faPuzzlePiece}
          title='No add-ons found'
          description='Try a different search, a different type, or the other source.'
        />
      )}

      {!searching && !searchError && query.trim().length < 2 && !results && (
        <EmptyState
          icon={faPuzzlePiece}
          title='Find an add-on'
          description='Search Modrinth for a mod, plugin or modpack. Matching versions are picked for this server automatically.'
        />
      )}

      <div className='flex flex-col gap-3'>
        {results?.projects.map((project) => (
          <TitleCard
            key={`${project.source}:${project.id}`}
            title={project.title}
            icon={project.iconUrl ? undefined : faPuzzlePiece}
          >
            <div className='flex gap-3 items-start'>
              {project.iconUrl && (
                <img src={project.iconUrl} alt='' className='w-12 h-12 rounded-md shrink-0' />
              )}
              <div className='min-w-0 flex-1'>
                <Group gap='xs' mb='xs'>
                  <Badge>{project.projectType}</Badge>
                  <Badge>{project.source}</Badge>
                  <Text c='dimmed' size='sm'>
                    <FontAwesomeIcon icon={faDownload} /> {project.downloads.toLocaleString()} downloads
                  </Text>
                  {project.author && (
                    <Text c='dimmed' size='sm'>
                      by {project.author}
                    </Text>
                  )}
                </Group>
                {project.description && (
                  <Text size='sm' lineClamp={2}>
                    {project.description}
                  </Text>
                )}

                {selectedProject?.id === project.id && (
                  <div className='mt-3'>
                    {versionsLoading ? (
                      <Spinner.Centered />
                    ) : versionsError ? (
                      <ErrorState error={httpErrorToHuman(versionsError)} onRetry={() => openVersions(project)} />
                    ) : (
                      <>
                        <Select
                          label='Version'
                          data={versions.map((version) => ({
                            value: version.id,
                            label: `${version.versionNumber} — ${version.gameVersions.join(', ') || 'any version'}${version.compatible ? '' : ' (not compatible with this server)'}`,
                            disabled: !version.compatible,
                          }))}
                          value={selectedVersionId}
                          onChange={setSelectedVersionId}
                          searchable
                        />
                        {selectedVersion && (
                          <Text c='dimmed' size='sm' mt='xs'>
                            Installs to {DESTINATION_BY_TYPE[project.projectType] ?? '/'}
                            {project.projectType === 'modpack' &&
                              ' — a .mrpack is downloaded to the server root; extracting the pack is not automated, run the pack installer for this server afterwards.'}
                          </Text>
                        )}
                        <Group mt='sm'>
                          <ServerCan action='files.create'>
                            <Button
                              color='blue'
                              loading={installing}
                              disabled={!selectedVersion || !selectedVersion.compatible || !canInstall}
                              onClick={doInstall}
                            >
                              {t('common.button.install', {})}
                            </Button>
                          </ServerCan>
                          <Button variant='default' onClick={() => setSelectedProject(null)}>
                            {t('common.button.cancel', {})}
                          </Button>
                        </Group>
                      </>
                    )}
                  </div>
                )}
              </div>

              {selectedProject?.id !== project.id && (
                <Button variant='default' onClick={() => openVersions(project)}>
                  View versions
                </Button>
              )}
            </div>
          </TitleCard>
        ))}
      </div>

      <Title order={2} mt='xl' mb='md'>
        Installed
      </Title>
      {installedLoading ? (
        <Spinner.Centered />
      ) : !installed || installed.total === 0 ? (
        <EmptyState
          icon={faPuzzlePiece}
          title='Nothing installed yet'
          description='Add-ons you install from this page will be listed here.'
        />
      ) : (
        <div className='flex flex-col gap-2'>
          {installed.data.map((activity) => {
            const data = (activity.data ?? {}) as Record<string, unknown>;
            return (
              <TitleCard
                key={`${activity.created.getTime()}-${String(data.file ?? '')}`}
                title={String(data.file ?? 'Add-on')}
              >
                <Group gap='xs'>
                  <Badge>{String(data.source ?? '')}</Badge>
                  <Badge>{String(data.project_type ?? '')}</Badge>
                  <Text c='dimmed' size='sm'>
                    {String(data.version_number ?? '')} → {String(data.destination ?? '')}
                  </Text>
                  <Text c='dimmed' size='sm'>
                    {activity.created.toLocaleString()}
                  </Text>
                </Group>
              </TitleCard>
            );
          })}
        </div>
      )}
    </ServerContentContainer>
  );
}
