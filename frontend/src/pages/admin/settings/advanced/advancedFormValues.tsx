import { z } from 'zod';
import type { FieldDef } from '@/elements/form-engine/index.ts';
import { adminSettingsAdvancedSchema } from '@/lib/schemas/admin/settings.ts';
import { useTranslations } from '@/providers/TranslationProvider.tsx';

type AdvancedSettingsFormValues = z.infer<typeof adminSettingsAdvancedSchema>;

/**
 * The panel's built-in SSRF block list — a copy of
 * `DEFAULT_BLOCKED_CIDRS` in `shared/src/settings/advanced.rs` (which
 * itself copies `Env::default_blocked_cidrs`). Keep the three in sync.
 * A `null` list from the API (unset) is hydrated to this list, so saving
 * the tab without touching it does not silently disable SSRF protection.
 */
export const DEFAULT_BLOCKED_CIDRS: string[] = [
  '0.0.0.0/8',
  '127.0.0.0/8',
  '10.0.0.0/8',
  '100.64.0.0/10',
  '172.16.0.0/12',
  '192.168.0.0/16',
  '169.254.0.0/16',
  '192.0.0.0/24',
  '198.18.0.0/15',
  '224.0.0.0/4',
  '240.0.0.0/4',
  '::/128',
  '::1/128',
  'fe80::/10',
  'fc00::/7',
  '2002::/16',
  'ff00::/8',
];

export const advancedSettingsEmptyFormValues: AdvancedSettingsFormValues = {
  passwordAlgorithm: null,
  passwordBcryptCost: null,
  passwordArgon2MemoryMib: null,
  passwordArgon2Iterations: null,
  blockedCidrs: [],
  trustedProxies: [],
  requestLogLimit: null,
  sentryUrl: null,
  sentryTracingSampleRate: null,
  enableWingsProxy: false,
  useDecryptionCache: false,
  useInternalCache: true,
};

export const advancedSettingsToFormValues = (
  advanced: AdvancedSettingsFormValues,
): Partial<AdvancedSettingsFormValues> => ({
  ...advanced,
  // A `null` list from the API means "unset": show the effective default
  // instead of an empty list, so saving without touching the list does not
  // silently disable SSRF protection (blocked) or change proxy trust.
  blockedCidrs: advanced.blockedCidrs ?? [...DEFAULT_BLOCKED_CIDRS],
  trustedProxies: advanced.trustedProxies ?? [],
  // Switches hydrate `null` to the built-in code defaults. That is the
  // position shown — not necessarily the value in force: when a field is
  // unset the environment variable governs, and the value in force is
  // shown in the Environment section below. Saving the tab stores an
  // explicit value for all three switches.
  enableWingsProxy: advanced.enableWingsProxy ?? false,
  useDecryptionCache: advanced.useDecryptionCache ?? false,
  useInternalCache: advanced.useInternalCache ?? true,
});

export function useAdvancedSettingsFormFields(): FieldDef<AdvancedSettingsFormValues>[] {
  const { t } = useTranslations();

  return [
    {
      type: 'divider',
      name: 'passwordDivider',
      label: t('pages.admin.settings.tabs.advanced.page.section.password', {}),
    },
    {
      type: 'select',
      name: 'passwordAlgorithm',
      label: t('pages.admin.settings.tabs.advanced.page.form.passwordAlgorithm', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.passwordAlgorithmDescription', {}),
      props: {
        clearable: true,
        placeholder: t('pages.admin.settings.tabs.advanced.page.form.passwordAlgorithmPlaceholder', {}),
      },
      options: [
        {
          label: t('pages.admin.settings.tabs.advanced.page.enum.passwordAlgorithm.bcrypt', {}),
          value: 'bcrypt',
        },
        {
          label: t('pages.admin.settings.tabs.advanced.page.enum.passwordAlgorithm.argon2id', {}),
          value: 'argon2id',
        },
      ],
    },
    {
      type: 'number',
      name: 'passwordBcryptCost',
      label: t('pages.admin.settings.tabs.advanced.page.form.passwordBcryptCost', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.passwordBcryptCostDescription', {}),
      when: (values) => values.passwordAlgorithm === 'bcrypt',
    },
    {
      type: 'number',
      name: 'passwordArgon2MemoryMib',
      label: t('pages.admin.settings.tabs.advanced.page.form.passwordArgon2MemoryMib', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.passwordArgon2MemoryMibDescription', {}),
      when: (values) => values.passwordAlgorithm === 'argon2id',
    },
    {
      type: 'number',
      name: 'passwordArgon2Iterations',
      label: t('pages.admin.settings.tabs.advanced.page.form.passwordArgon2Iterations', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.passwordArgon2IterationsDescription', {}),
      when: (values) => values.passwordAlgorithm === 'argon2id',
    },
    {
      type: 'divider',
      name: 'networkingDivider',
      label: t('pages.admin.settings.tabs.advanced.page.section.networking', {}),
    },
    {
      type: 'tags',
      name: 'blockedCidrs',
      label: t('pages.admin.settings.tabs.advanced.page.form.blockedCidrs', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.blockedCidrsDescription', {}),
      placeholder: 'e.g. 10.0.0.0/8',
    },
    {
      type: 'tags',
      name: 'trustedProxies',
      label: t('pages.admin.settings.tabs.advanced.page.form.trustedProxies', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.trustedProxiesDescription', {}),
      placeholder: 'e.g. 10.0.0.0/8',
    },
    {
      type: 'divider',
      name: 'loggingDivider',
      label: t('pages.admin.settings.tabs.advanced.page.section.logging', {}),
    },
    {
      type: 'number',
      name: 'requestLogLimit',
      label: t('pages.admin.settings.tabs.advanced.page.form.requestLogLimit', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.requestLogLimitDescription', {}),
    },
    {
      type: 'divider',
      name: 'sentryDivider',
      label: t('pages.admin.settings.tabs.advanced.page.section.sentry', {}),
    },
    {
      type: 'text',
      name: 'sentryUrl',
      label: t('pages.admin.settings.tabs.advanced.page.form.sentryUrl', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.sentryUrlDescription', {}),
    },
    {
      type: 'number',
      name: 'sentryTracingSampleRate',
      label: t('pages.admin.settings.tabs.advanced.page.form.sentryTracingSampleRate', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.sentryTracingSampleRateDescription', {}),
    },
    {
      type: 'divider',
      name: 'featuresDivider',
      label: t('pages.admin.settings.tabs.advanced.page.section.features', {}),
    },
    {
      type: 'switch',
      name: 'enableWingsProxy',
      label: t('pages.admin.settings.tabs.advanced.page.form.enableWingsProxy', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.enableWingsProxyDescription', {}),
    },
    {
      type: 'switch',
      name: 'useDecryptionCache',
      label: t('pages.admin.settings.tabs.advanced.page.form.useDecryptionCache', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.useDecryptionCacheDescription', {}),
    },
    {
      type: 'switch',
      name: 'useInternalCache',
      label: t('pages.admin.settings.tabs.advanced.page.form.useInternalCache', {}),
      description: t('pages.admin.settings.tabs.advanced.page.form.useInternalCacheDescription', {}),
    },
  ];
}
