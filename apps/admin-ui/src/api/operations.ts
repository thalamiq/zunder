import { getFetcher } from "./client";

export interface OperationParameter {
  name: string;
  use: "in" | "out" | "both";
  min: number;
  max: string;
  type?: string;
  searchType?: string;
  documentation?: string;
  part?: OperationParameter[];
}

export interface OperationMetadata {
  name: string;
  code: string;
  system: boolean;
  type_level: boolean;
  type_contexts: string[];
  instance: boolean;
  parameters: OperationParameter[];
  affects_state: boolean;
}

export async function fetchOperations(): Promise<OperationMetadata[]> {
  return getFetcher<OperationMetadata[]>("/admin/operations");
}
