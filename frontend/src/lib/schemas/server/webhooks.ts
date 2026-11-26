import { z } from 'zod';

export const serverWebhookKindSchema = z.enum(['discord', 'generic']);
export const serverWebhookEventSchema = z.enum(['server_crashed', 'server_offline', 'server_recovered']);
export const serverAlertKindSchema = z.enum(['crashed', 'offline', 'recovered']);

export const serverWebhookSchema = z.looseObject({
  uuid: z.string(),
  serverUuid: z.string().nullable(),
  name: z.string().min(1).max(255),
  kind: serverWebhookKindSchema,
  url: z.string().min(1).max(2048),
  events: z.array(serverWebhookEventSchema),
  enabled: z.boolean(),
  hasSecret: z.boolean(),
  lastTriggeredAt: z.coerce.date().nullable(),
  failureCount: z.number().int().min(0),
  created: z.coerce.date(),
});

export const serverWebhookCreateSchema = z.object({
  name: z.string().min(1).max(255),
  kind: serverWebhookKindSchema,
  url: z.string().url().max(2048),
  events: z.array(serverWebhookEventSchema).min(1),
  enabled: z.boolean(),
});

export const serverWebhookUpdateSchema = serverWebhookCreateSchema.partial();

export const serverWebhookCreateResponseSchema = z.object({
  webhook: serverWebhookSchema,
  // HMAC secret for generic webhooks, returned exactly once at
  // creation. Empty string for Discord webhooks.
  secret: z.string(),
});

export const serverAlertSchema = z.looseObject({
  uuid: z.string(),
  serverUuid: z.string(),
  kind: serverAlertKindSchema,
  crashId: z.number().int().nullable(),
  exitCode: z.number().int().nullable(),
  oomKilled: z.boolean(),
  lastConsole: z.string().nullable(),
  detectedAt: z.coerce.date(),
  created: z.coerce.date(),
});

export const adminServerAlertSchema = z.looseObject({
  uuid: z.string(),
  serverUuid: z.string(),
  kind: serverAlertKindSchema,
  crashId: z.number().int().nullable(),
  exitCode: z.number().int().nullable(),
  oomKilled: z.boolean(),
  lastConsole: z.string().nullable(),
  detectedAt: z.coerce.date(),
  created: z.coerce.date(),
  serverName: z.string(),
  serverUuidShort: z.string(),
});

export type ServerWebhook = z.infer<typeof serverWebhookSchema>;
export type ServerWebhookCreate = z.infer<typeof serverWebhookCreateSchema>;
export type ServerAlert = z.infer<typeof serverAlertSchema>;
export type AdminServerAlert = z.infer<typeof adminServerAlertSchema>;
