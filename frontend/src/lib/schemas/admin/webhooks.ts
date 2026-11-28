import { z } from 'zod';

export const WEBHOOK_EVENTS = [
  'server.created',
  'server.started',
  'server.stopped',
  'server.crashed',
  'server.install.started',
  'server.install.completed',
  'server.transfer.started',
  'server.transfer.completed',
  'backup.finished',
  'backup.restore.finished',
  'backup.deleted',
  'user.created',
] as const;

export const webhookSchema = z.looseObject({
  uuid: z.string(),
  name: z.string(),
  url: z.string(),
  events: z.array(z.string()),
  enabled: z.boolean(),
  hasSecret: z.boolean(),
  createdByUuid: z.string().nullable(),
  created: z.coerce.date(),
  updated: z.coerce.date(),
});

export const webhookCreateSchema = z.object({
  name: z.string().min(1).max(128),
  url: z.string().url().max(2048),
  events: z.array(z.string()).min(1).max(32),
  enabled: z.boolean(),
});

export const webhookUpdateSchema = webhookCreateSchema.partial();

export const webhookDeliverySchema = z.looseObject({
  uuid: z.string(),
  subscriptionUuid: z.string(),
  event: z.string(),
  attempt: z.number(),
  statusCode: z.number().nullable(),
  success: z.boolean(),
  error: z.string().nullable(),
  created: z.coerce.date(),
});

export const webhookTestResultSchema = z.looseObject({
  success: z.boolean(),
  statusCode: z.number().nullable(),
  error: z.string().nullable(),
  attempts: z.number(),
});
