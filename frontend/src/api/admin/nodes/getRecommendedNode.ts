import { z } from 'zod';
import { axiosInstance } from '@/api/axios.ts';
import { adminNodeRecommendationSchema } from '@/lib/schemas/admin/nodes.ts';
import { parseFromApi } from '@/lib/serialization/api-transform.ts';

export interface RecommendedNodeParams {
  memory: number;
  cpu: number;
  disk: number;
  locationUuids: string[];
  allowOverallocation?: boolean;
}

export default async (
  params: RecommendedNodeParams,
): Promise<z.infer<typeof adminNodeRecommendationSchema>> => {
  const search = new URLSearchParams();
  search.set('memory', String(params.memory));
  search.set('cpu', String(params.cpu));
  search.set('disk', String(params.disk));
  for (const uuid of params.locationUuids) {
    search.append('location_uuid', uuid);
  }
  if (params.allowOverallocation) {
    search.set('allow_overallocation', 'true');
  }

  const { data } = await axiosInstance.get(`/api/admin/nodes/recommended?${search.toString()}`);
  return parseFromApi(adminNodeRecommendationSchema, data);
};
