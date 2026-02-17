import { deleteFetcher, getFetcher, postFetcher } from "./client";

export interface JobRecord {
  id: string;
  jobType: string;
  status: string;
  priority: string;
  parameters: unknown;
  progress?: unknown;
  processedItems: number;
  totalItems: number | null;
  progressPercent: number | null;
  errorMessage: string | null;
  lastErrorAt: string | null;
  scheduledAt: string | null;
  cancelRequested: boolean;
  createdAt: string;
  startedAt: string | null;
  completedAt: string | null;
  workerId: string | null;
  retryCount: number;
}

export interface JobDetailRecord extends JobRecord {
  retryPolicy: unknown;
}

export interface ListJobsResponse {
  jobs: JobRecord[];
  total: number;
  limit: number;
  offset: number;
}

export interface QueueHealthResponse {
  status: string;
  backend: string;
  stats_24h: {
    total: number;
    pending: number;
    running: number;
    completed: number;
    failed: number;
    cancelled: number;
  };
}

export interface CancelJobResponse {
  cancelled: boolean;
  jobId: string;
}

export interface CleanupJobsResponse {
  deleted: number;
  days: number;
}

export const listJobs = async ({
  jobType,
  status,
  limit,
  offset,
}: {
  jobType?: string;
  status?: string;
  limit?: number;
  offset?: number;
} = {}): Promise<ListJobsResponse> => {
  const params = new URLSearchParams();
  if (jobType) params.set("jobType", jobType);
  if (status) params.set("status", status);
  if (typeof limit === "number") params.set("limit", String(limit));
  if (typeof offset === "number") params.set("offset", String(offset));

  const query = params.toString();
  const url = query ? `/admin/jobs?${query}` : "/admin/jobs";
  return getFetcher<ListJobsResponse>(url);
};

export const getJob = async (id: string): Promise<JobDetailRecord> => {
  return getFetcher<JobDetailRecord>(`/admin/jobs/${id}`);
};

export const cancelJob = async (id: string): Promise<CancelJobResponse> => {
  return postFetcher<CancelJobResponse>(`/admin/jobs/${id}/cancel`, {});
};

export const cleanupOldJobs = async ({
  days,
}: {
  days?: number;
} = {}): Promise<CleanupJobsResponse> => {
  const params = new URLSearchParams();
  if (typeof days === "number") params.set("days", String(days));
  const query = params.toString();
  const url = query ? `/admin/jobs/cleanup?${query}` : "/admin/jobs/cleanup";
  return postFetcher<CleanupJobsResponse>(url, {});
};

export const deleteJob = async (id: string): Promise<void> => {
  return deleteFetcher(`/admin/jobs/${id}`);
};

export const getQueueHealth = async (): Promise<QueueHealthResponse> => {
  return getFetcher<QueueHealthResponse>("/admin/jobs/health");
};
