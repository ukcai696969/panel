import { z } from 'zod';
import { adminServerFeatureLimitsSchema, adminServerLimitsSchema } from '@/lib/schemas/admin/servers.ts';
import { nullableString } from '@/lib/serialization/transformers.ts';

export const serverTemplateSchema = z.looseObject({
  uuid: z.string(),
  name: z.string().min(1).max(255),
  description: z.preprocess(nullableString, z.string().max(1024).nullable()),
  ownerUuid: z.string().nullable(),
  isGlobal: z.boolean(),
  sourceServerUuid: z.string().nullable(),
  eggUuid: z.uuid(),
  eggName: z.string(),
  allocationCount: z.number().min(0),
  scheduleCount: z.number().min(0),
  created: z.coerce.date(),
  updated: z.coerce.date(),
});

export const serverTemplateCloneSchema = z.object({
  name: z.string().min(1).max(255),
  description: z.string().max(1024).optional(),
  includeFiles: z.boolean().default(false),
  includeSchedules: z.boolean().default(true),
  startOnCompletion: z.boolean().default(false),
});

export const serverTemplateCreateSchema = z.object({
  serverUuid: z.uuid(),
  name: z.string().min(1).max(255),
  description: z.string().max(1024).optional(),
});

export const serverTemplateInstantiateSchema = z.object({
  nodeUuid: z.uuid(),
  allocationUuid: z.uuid().nullable().optional(),
  allocationUuids: z.array(z.uuid()).default([]),
  startOnCompletion: z.boolean().default(false),
  name: z.string().min(1).max(255),
  description: z.string().max(1024).optional(),
});

export type ServerTemplate = z.infer<typeof serverTemplateSchema>;

/** Admin view of a template, including the full snapshot needed to prefill the create form. */
export const adminServerTemplateSchema = z.looseObject({
  ...serverTemplateSchema.shape,
  snapshot: z.looseObject({
    eggUuid: z.uuid(),
    backupConfigurationUuid: z.uuid().nullable(),
    variables: z.record(z.string(), z.string()),
    limits: z.lazy(() => adminServerLimitsSchema),
    pinnedCpus: z.array(z.number()),
    featureLimits: z.lazy(() => adminServerFeatureLimitsSchema),
    startup: z.string(),
    image: z.string(),
    labels: z.record(z.string(), z.string()),
    timezone: z.preprocess(nullableString, z.string().nullable()),
    trash: z.unknown().nullable(),
    hugepagesPassthroughEnabled: z.boolean(),
    kvmPassthroughEnabled: z.boolean(),
    allocationCount: z.number(),
    schedules: z.array(z.unknown()),
    sourceServerUuid: z.string().nullable(),
  }),
});

export type AdminServerTemplate = z.infer<typeof adminServerTemplateSchema>;
export type ServerTemplateClone = z.infer<typeof serverTemplateCloneSchema>;
export type ServerTemplateCreate = z.infer<typeof serverTemplateCreateSchema>;
export type ServerTemplateInstantiate = z.infer<typeof serverTemplateInstantiateSchema>;
