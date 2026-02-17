import { ConfigCategory } from "@/api/config";

  export const queryKeys = {
  metadata: (mode: "full" | "normative" | "terminology" = "full") => [
    "metadata",
    mode,
  ],
  resources: ["resources"],
  jobs: (
    jobType?: string,
    status?: string,
    limit?: number,
    offset?: number
  ) => ["jobs", jobType, status, limit, offset],
  job: (id: string) => ["job", id],
  queueHealth: ["queueHealth"],
  searchParameterIndexingStatus: (resourceType?: string) => [
    "searchParameterIndexingStatus",
    resourceType,
  ],
  searchIndexTableStatus: ["searchIndexTableStatus"],
  searchHashCollisions: ["searchHashCollisions"],
  fhirSearchParameters: (count: number, offset: number) => [
    "fhirSearchParameters",
    count,
    offset,
  ],
  adminSearchParameters: (
    q?: string,
    status?: string,
    type?: string,
    resourceType?: string,
    limit?: number,
    offset?: number
  ) => ["adminSearchParameters", q, status, type, resourceType, limit, offset],
  packages: (status?: string, limit?: number, offset?: number) => [
    "packages",
    status,
    limit,
    offset,
  ],
  package: (id: string) => ["package", id],
  packageResources: (
    packageId: number,
    includeDeleted: boolean,
    resourceType?: string,
    limit?: number,
    offset?: number
  ) => [
    "packageResources",
    packageId,
    includeDeleted,
    resourceType,
    limit,
    offset,
  ],
  fhir: (path: string) => ["fhir", path],
  health: ["health"],
  auditEvents: (
    action?: string,
    outcome?: string,
    resourceType?: string,
    resourceId?: string,
    patientId?: string,
    clientId?: string,
    userId?: string,
    requestId?: string,
    limit?: number,
    offset?: number
  ) => [
    "auditEvents",
    action,
    outcome,
    resourceType,
    resourceId,
    patientId,
    clientId,
    userId,
    requestId,
    limit,
    offset,
  ],
  auditEvent: (id: number) => ["auditEvent", id],
  runtimeConfig: (category: ConfigCategory) => ["runtimeConfig", category],
  runtimeConfigAudit: (key?: string, limit?: number, offset?: number) => ["runtimeConfigAudit", key, limit, offset],
  compartmentMemberships: ["compartmentMemberships"],
  resourceReferences: (resourceType: string, id: string) => [
    "resourceReferences",
    resourceType,
    id,
  ],
};
