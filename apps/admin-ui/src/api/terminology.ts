import { getFetcher } from "./client";

export interface CodeSystemSummary {
  url: string;
  conceptCount: number;
}

export interface ClosureTableSummary {
  name: string;
  currentVersion: number;
  requiresReinit: boolean;
  conceptCount: number;
  relationCount: number;
}

export interface TerminologySummary {
  codesystems: CodeSystemSummary[];
  totalConcepts: number;
  cachedExpansions: number;
  activeExpansions: number;
  valuesetCount: number;
  conceptmapCount: number;
  closureTables: ClosureTableSummary[];
}

export async function fetchTerminologySummary(): Promise<TerminologySummary> {
  return getFetcher<TerminologySummary>("/admin/terminology");
}
