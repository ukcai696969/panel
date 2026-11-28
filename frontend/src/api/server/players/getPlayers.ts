import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { serverPlayersSchema } from '@/lib/schemas/server/players.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';

export default async (uuid: string, lines = 200): Promise<z.infer<typeof serverPlayersSchema>> => {
  const { data } = await axiosInstance.get(`/api/client/servers/${uuid}/players`, {
    params: { lines },
  });
  return parseFromApi(serverPlayersSchema, data);
};
