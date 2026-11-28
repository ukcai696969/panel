import { axiosInstance } from '@/api/axios.ts';
import { downloadBlob } from '@/lib/download/download.ts';

export type AdminActivityExportFormat = 'csv' | 'json';
export type AdminActivityExportType = 'admin' | 'user' | 'team' | 'server' | 'all';

export interface AdminActivityExportOptions {
  format: AdminActivityExportFormat;
  /** Inclusive lower bound (date-only or full ISO datetime). */
  from?: string;
  /** Inclusive upper bound (date-only or full ISO datetime). */
  to?: string;
  type?: AdminActivityExportType;
}

/** Downloads the admin activity export as a file. */
export default async (options: AdminActivityExportOptions): Promise<void> => {
  const params: Record<string, string> = { format: options.format };
  if (options.from) params.from = options.from;
  if (options.to) params.to = options.to;
  if (options.type) params.type = options.type;

  const response = await axiosInstance.get('/api/admin/activity/export', {
    params,
    responseType: 'blob',
  });

  const stamp = new Date().toISOString().slice(0, 19).replace(/[-:T]/g, '');
  downloadBlob(
    response.data as Blob,
    `activity-export-${stamp}.${options.format}`,
  );
};
