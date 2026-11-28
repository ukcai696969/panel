import { axiosInstance } from '@/api/axios.ts';

/**
 * Fetches the recent console log as plain text through the existing client
 * API (`GET /api/client/servers/{server}/logs`), which proxies the Wings
 * logs endpoint. Requires `control.read-console`. Used by the "Download
 * console log" button to get the full log, rather than only the lines the
 * terminal has shown since the page was opened.
 */
export default async (uuid: string, lines = 1000): Promise<string> => {
  const { data } = await axiosInstance.get(`/api/client/servers/${uuid}/logs`, {
    params: { lines },
    responseType: 'text',
  });
  return data;
};
