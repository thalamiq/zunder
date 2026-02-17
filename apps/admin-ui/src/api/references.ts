import { getFetcher } from "./client";

export interface ReferenceEdge {
  sourceType: string;
  sourceId: string;
  parameterName: string;
  targetType: string;
  targetId: string;
  display: string | null;
}

export interface ResourceReferencesResponse {
  resourceType: string;
  resourceId: string;
  outgoing: ReferenceEdge[];
  incoming: ReferenceEdge[];
}

export const fetchResourceReferences = async (
  resourceType: string,
  id: string,
): Promise<ResourceReferencesResponse> => {
  return getFetcher<ResourceReferencesResponse>(
    `/admin/resources/${encodeURIComponent(resourceType)}/${encodeURIComponent(id)}/references`,
  );
};
