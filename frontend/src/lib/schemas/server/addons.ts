import { z } from 'zod';

export const addonSourceSchema = z.enum(['modrinth', 'curseforge']);

export const addonProjectSchema = z.looseObject({
  source: addonSourceSchema,
  id: z.string(),
  slug: z.string().nullable(),
  title: z.string(),
  description: z.string().nullable(),
  author: z.string().nullable(),
  iconUrl: z.string().nullable(),
  projectType: z.string(),
  downloads: z.number(),
  gameVersions: z.array(z.string()),
  loaders: z.array(z.string()),
});

export const addonFileSchema = z.looseObject({
  url: z.string(),
  filename: z.string(),
  primary: z.boolean(),
  size: z.number().nullable(),
});

export const addonVersionSchema = z.looseObject({
  id: z.string(),
  projectId: z.string(),
  name: z.string(),
  versionNumber: z.string(),
  gameVersions: z.array(z.string()),
  loaders: z.array(z.string()),
  files: z.array(addonFileSchema),
  datePublished: z.string().nullable(),
  compatible: z.boolean(),
});

export const addonSearchContextSchema = z.looseObject({
  isMinecraft: z.boolean(),
  gameVersion: z.string().nullable(),
  loaders: z.array(z.string()),
});

export const addonSearchResultSchema = z.looseObject({
  projects: z.array(addonProjectSchema),
  total: z.number(),
  page: z.number(),
  perPage: z.number(),
  context: addonSearchContextSchema,
  source: addonSourceSchema,
});

export const addonVersionsResultSchema = z.looseObject({
  versions: z.array(addonVersionSchema),
  gameVersion: z.string().nullable(),
  loaders: z.array(z.string()),
  source: addonSourceSchema,
});

export const addonInstallResultSchema = z.looseObject({
  identifier: z.string().nullable(),
});
