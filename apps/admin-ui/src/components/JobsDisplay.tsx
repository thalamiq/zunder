import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { Button } from "@thalamiq/ui/components/button";
import { Input } from "@thalamiq/ui/components/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@thalamiq/ui/components/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@thalamiq/ui/components/table";
import {
  Clock,
  RefreshCw,
  Trash2,
  XCircle,
  CheckCircle2,
  AlertTriangle,
  Loader2,
  MoreVertical,
} from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@thalamiq/ui/components/dropdown-menu";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import JsonViewer from "@/components/JsonViewer";
import {
  cancelJob,
  cleanupOldJobs,
  deleteJob,
  getJob,
  getQueueHealth,
  listJobs,
  JobRecord,
} from "@/api/jobs";
import { queryKeys } from "@/api/query-keys";
import { formatDateTime, formatDateTimeFull } from "@/lib/utils";

const formatNumber = (num: number): string =>
  new Intl.NumberFormat().format(num);

const normalizeStatus = (status: string): string => status.trim().toLowerCase();

const isTerminalStatus = (status: string): boolean => {
  const s = normalizeStatus(status);
  return s === "completed" || s === "failed" || s === "cancelled";
};

const getStatusBadge = (status: string) => {
  const s = normalizeStatus(status);

  if (s === "completed") {
    return (
      <Badge className="bg-success">
        <CheckCircle2 className="h-3 w-3 mr-1" />
        Completed
      </Badge>
    );
  }
  if (s === "running") {
    return (
      <Badge className="bg-info">
        <Loader2 className="h-3 w-3 mr-1 animate-spin" />
        Running
      </Badge>
    );
  }
  if (s === "pending") {
    return (
      <Badge variant="secondary">
        <Clock className="h-3 w-3 mr-1" />
        Pending
      </Badge>
    );
  }
  if (s === "retrying") {
    return (
      <Badge className="bg-amber-500 hover:bg-amber-600">
        <RefreshCw className="h-3 w-3 mr-1" />
        Retrying
      </Badge>
    );
  }
  if (s === "failed") {
    return (
      <Badge variant="destructive">
        <XCircle className="h-3 w-3 mr-1" />
        Failed
      </Badge>
    );
  }
  if (s === "cancelled") {
    return (
      <Badge variant="outline">
        <XCircle className="h-3 w-3 mr-1" />
        Cancelled
      </Badge>
    );
  }
  return <Badge variant="outline">{status}</Badge>;
};

const progressPercent = (job: JobRecord): number | null => {
  if (typeof job.progressPercent === "number") {
    return Math.max(0, Math.min(100, job.progressPercent));
  }
  if (job.totalItems && job.totalItems > 0) {
    return Math.max(
      0,
      Math.min(100, (job.processedItems / job.totalItems) * 100)
    );
  }
  return null;
};

const ProgressBar = ({ value }: { value: number }) => (
  <div className="w-full">
    <div className="h-2 w-full rounded-full bg-muted overflow-hidden">
      <div
        className="h-full bg-primary transition-all"
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  </div>
);

const JobsDisplay = () => {
  const queryClient = useQueryClient();

  const [limit, setLimit] = useState(50);
  const [offset, setOffset] = useState(0);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(
    undefined
  );
  const [jobTypeFilter, setJobTypeFilter] = useState<string | undefined>(
    undefined
  );
  const [search, setSearch] = useState("");
  const [detailsJobId, setDetailsJobId] = useState<string | null>(null);
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [cleanupDays, setCleanupDays] = useState(30);

  const jobsQuery = useQuery({
    queryKey: queryKeys.jobs(jobTypeFilter, statusFilter, limit, offset),
    queryFn: () =>
      listJobs({
        jobType: jobTypeFilter,
        status: statusFilter,
        limit,
        offset,
      }),
    refetchInterval: (q) => {
      const data = q.state.data;
      const hasActive = data?.jobs?.some(
        (job) =>
          !isTerminalStatus(job.status) &&
          normalizeStatus(job.status) !== "cancelled"
      );
      return hasActive ? 5000 : 30000;
    },
  });

  const healthQuery = useQuery({
    queryKey: queryKeys.queueHealth,
    queryFn: () => getQueueHealth(),
    refetchInterval: 30000,
  });

  const jobDetailsQuery = useQuery({
    queryKey: detailsJobId ? queryKeys.job(detailsJobId) : ["job", "none"],
    queryFn: () => {
      if (!detailsJobId) {
        throw new Error("No job selected");
      }
      return getJob(detailsJobId);
    },
    enabled: !!detailsJobId,
    refetchInterval: (q) => {
      const data = q.state.data;
      if (!data) return false;
      return isTerminalStatus(data.status) ? 30000 : 5000;
    },
  });

  const cancelMutation = useMutation({
    mutationFn: (jobId: string) => cancelJob(jobId),
    onSuccess: (_data, jobId) => {
      toast.success("Cancellation requested");
      queryClient.invalidateQueries({ queryKey: ["jobs"] });
      queryClient.invalidateQueries({ queryKey: queryKeys.job(jobId) });
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to cancel job");
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (jobId: string) => deleteJob(jobId),
    onSuccess: (_data, jobId) => {
      toast.success("Job deleted");
      queryClient.invalidateQueries({ queryKey: ["jobs"] });
      queryClient.invalidateQueries({ queryKey: queryKeys.queueHealth });
      if (detailsJobId === jobId) setDetailsJobId(null);
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to delete job");
    },
  });

  const cleanupMutation = useMutation({
    mutationFn: (days: number) => cleanupOldJobs({ days }),
    onSuccess: (data) => {
      toast.success(`Deleted ${formatNumber(data.deleted)} job(s)`);
      queryClient.invalidateQueries({ queryKey: ["jobs"] });
      queryClient.invalidateQueries({ queryKey: queryKeys.queueHealth });
      setCleanupOpen(false);
    },
    onError: (err) => {
      toast.error(
        err instanceof Error ? err.message : "Failed to cleanup jobs"
      );
    },
  });

  const allJobs = useMemo(
    () => jobsQuery.data?.jobs ?? [],
    [jobsQuery.data?.jobs]
  );
  const total = jobsQuery.data?.total ?? 0;

  const uniqueStatuses = useMemo(() => {
    const statuses = new Set(allJobs.map((j) => j.status));
    return Array.from(statuses).sort((a, b) =>
      normalizeStatus(a).localeCompare(normalizeStatus(b))
    );
  }, [allJobs]);

  const uniqueJobTypes = useMemo(() => {
    const types = new Set(allJobs.map((j) => j.jobType));
    return Array.from(types).sort((a, b) => a.localeCompare(b));
  }, [allJobs]);

  const filteredJobs = useMemo(() => {
    if (!search.trim()) return allJobs;
    const needle = search.trim().toLowerCase();
    return allJobs.filter((job) => {
      return (
        job.id.toLowerCase().includes(needle) ||
        job.jobType.toLowerCase().includes(needle) ||
        job.status.toLowerCase().includes(needle) ||
        (job.workerId ?? "").toLowerCase().includes(needle) ||
        (job.errorMessage ?? "").toLowerCase().includes(needle)
      );
    });
  }, [allJobs, search]);

  const stats = useMemo(() => {
    const counts = filteredJobs.reduce((acc, job) => {
      const s = normalizeStatus(job.status);
      acc[s] = (acc[s] ?? 0) + 1;
      return acc;
    }, {} as Record<string, number>);
    return {
      running: counts["running"] ?? 0,
      pending: counts["pending"] ?? 0,
      retrying: counts["retrying"] ?? 0,
      completed: counts["completed"] ?? 0,
      failed: counts["failed"] ?? 0,
      cancelled: counts["cancelled"] ?? 0,
      total: filteredJobs.length,
    };
  }, [filteredJobs]);

  const currentPage = Math.floor(offset / limit) + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));

  if (jobsQuery.isPending) {
    return <LoadingArea />;
  }

  if (jobsQuery.isError) {
    return <ErrorArea error={jobsQuery.error} />;
  }

  return (
    <div className="space-y-4 p-6">
      <Card>
        <CardHeader>
          <div className="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
            <div>
              <CardTitle>Jobs</CardTitle>
              <CardDescription>
                Monitor background jobs, inspect details, cancel running work,
                and cleanup old entries
              </CardDescription>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                onClick={() => {
                  queryClient.invalidateQueries({ queryKey: ["jobs"] });
                  queryClient.invalidateQueries({
                    queryKey: queryKeys.queueHealth,
                  });
                  if (detailsJobId) {
                    queryClient.invalidateQueries({
                      queryKey: queryKeys.job(detailsJobId),
                    });
                  }
                }}
              >
                <RefreshCw className="w-4 h-4 mr-2" />
                Refresh
              </Button>
              <Button variant="secondary" onClick={() => setCleanupOpen(true)}>
                <Trash2 className="w-4 h-4 mr-2" />
                Cleanup
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-8 gap-4">
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Total (page)
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(stats.total)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Running
              </div>
              <div className="text-2xl font-bold text-blue-600">
                {formatNumber(stats.running)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Pending
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(stats.pending)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Retrying
              </div>
              <div className="text-2xl font-bold text-amber-600">
                {formatNumber(stats.retrying)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Completed
              </div>
              <div className="text-2xl font-bold text-success">
                {formatNumber(stats.completed)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Failed
              </div>
              <div className="text-2xl font-bold text-destructive">
                {formatNumber(stats.failed)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Cancelled
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(stats.cancelled)}
              </div>
            </div>
            <div className="col-span-2 md:col-span-1 lg:col-span-1">
              <div className="text-sm font-medium text-muted-foreground">
                Queue (24h)
              </div>
              {healthQuery.data ? (
                <div className="text-xs text-muted-foreground mt-1 space-y-0.5">
                  <div className="flex justify-between">
                    <span>Running</span>
                    <span className="font-mono">
                      {formatNumber(healthQuery.data.stats_24h.running)}
                    </span>
                  </div>
                  <div className="flex justify-between">
                    <span>Pending</span>
                    <span className="font-mono">
                      {formatNumber(healthQuery.data.stats_24h.pending)}
                    </span>
                  </div>
                  <div className="flex justify-between">
                    <span>Failed</span>
                    <span className="font-mono">
                      {formatNumber(healthQuery.data.stats_24h.failed)}
                    </span>
                  </div>
                </div>
              ) : (
                <div className="text-xs text-muted-foreground mt-1">—</div>
              )}
            </div>
          </div>
          <div className="mt-4 text-sm text-muted-foreground">
            Showing {formatNumber(allJobs.length)} of {formatNumber(total)} jobs
            (page {currentPage} of {totalPages})
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Job List</CardTitle>
          <CardDescription>Filter, inspect, and manage jobs</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col lg:flex-row gap-3">
            <div className="flex-1">
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search by id, type, status, worker, error..."
              />
            </div>
            <div className="flex gap-2 flex-wrap items-center">
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Status</span>
                <Select
                  value={statusFilter ?? "__all__"}
                  onValueChange={(value) => {
                    const next = value === "__all__" ? undefined : value;
                    setStatusFilter(next);
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[140px]">
                    <SelectValue placeholder="All" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All</SelectItem>
                    {uniqueStatuses.map((s) => (
                      <SelectItem key={s} value={s}>
                        {s}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Type</span>
                <Select
                  value={jobTypeFilter ?? "__all__"}
                  onValueChange={(value) => {
                    const next = value === "__all__" ? undefined : value;
                    setJobTypeFilter(next);
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[140px]">
                    <SelectValue placeholder="All" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All</SelectItem>
                    {uniqueJobTypes.map((t) => (
                      <SelectItem key={t} value={t}>
                        {t}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Page size</span>
                <Select
                  value={String(limit)}
                  onValueChange={(value) => {
                    setLimit(Number(value));
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[100px]">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {[25, 50, 100, 250].map((n) => (
                      <SelectItem key={n} value={String(n)}>
                        {n}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
          </div>

          {filteredJobs.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-10">
              No jobs match your filters.
            </div>
          ) : (
            <div className="rounded-md border overflow-hidden">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Status</TableHead>
                    <TableHead>Type</TableHead>
                    <TableHead>Progress</TableHead>
                    <TableHead>Created</TableHead>
                    <TableHead>Worker</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredJobs.map((job) => {
                    const pct = progressPercent(job);
                    const cancellable =
                      !isTerminalStatus(job.status) && !job.cancelRequested;

                    return (
                      <TableRow
                        key={job.id}
                        className="hover:bg-accent/30 cursor-pointer"
                        onClick={() => setDetailsJobId(job.id)}
                      >
                        <TableCell>{getStatusBadge(job.status)}</TableCell>
                        <TableCell>
                          <div className="space-y-1">
                            <div className="font-medium">{job.jobType}</div>
                            <div className="text-xs text-muted-foreground font-mono break-all">
                              {job.id}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell>
                          <div className="space-y-1">
                            {typeof pct === "number" ? (
                              <>
                                <div className="flex items-center justify-between text-xs text-muted-foreground">
                                  <span>
                                    {job.processedItems}
                                    {job.totalItems
                                      ? ` / ${job.totalItems}`
                                      : ""}
                                  </span>
                                  <span className="font-mono">
                                    {pct.toFixed(1)}%
                                  </span>
                                </div>
                                <ProgressBar value={pct} />
                              </>
                            ) : (
                              <div className="text-xs text-muted-foreground">
                                —
                              </div>
                            )}
                            {job.cancelRequested && (
                              <div className="text-xs text-amber-600 flex items-center gap-1">
                                <AlertTriangle className="w-3 h-3" />
                                Cancel requested
                              </div>
                            )}
                            {job.errorMessage && (
                              <div className="text-xs text-destructive line-clamp-2">
                                {job.errorMessage}
                              </div>
                            )}
                          </div>
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          <span title={formatDateTimeFull(job.createdAt)}>
                            {formatDateTime(job.createdAt)}
                          </span>
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {job.workerId ? (
                            <span className="font-mono">{job.workerId}</span>
                          ) : (
                            "—"
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          <DropdownMenu modal={false}>
                            <DropdownMenuTrigger asChild>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-8 w-8 p-0"
                                onClick={(e) => e.stopPropagation()}
                              >
                                <MoreVertical className="h-4 w-4" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                              <DropdownMenuItem
                                disabled={!cancellable || cancelMutation.isPending}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  cancelMutation.mutate(job.id);
                                }}
                              >
                                <XCircle className="mr-2 h-4 w-4" />
                                Cancel
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                disabled={
                                  deleteMutation.isPending ||
                                  !(isTerminalStatus(job.status) || job.cancelRequested)
                                }
                                onClick={(e) => {
                                  e.stopPropagation();
                                  deleteMutation.mutate(job.id);
                                }}
                              >
                                <Trash2 className="mr-2 h-4 w-4" />
                                Delete
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          )}

          <div className="flex items-center justify-between pt-2">
            <div className="text-xs text-muted-foreground">
              Offset {formatNumber(offset)} • Limit {formatNumber(limit)}
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={offset <= 0}
                onClick={() => setOffset(Math.max(0, offset - limit))}
              >
                Previous
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={offset + limit >= total}
                onClick={() => setOffset(offset + limit)}
              >
                Next
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <Dialog open={cleanupOpen} onOpenChange={setCleanupOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Cleanup old jobs</DialogTitle>
            <DialogDescription>
              Deletes completed, failed, and cancelled jobs older than the
              selected number of days.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <div className="flex items-center justify-between gap-3">
              <span className="text-sm font-medium">Days</span>
              <Input
                type="number"
                min={1}
                className="w-28"
                value={cleanupDays}
                onChange={(e) => setCleanupDays(Number(e.target.value))}
              />
            </div>
            <div className="text-xs text-muted-foreground">
              Tip: start with 7-30 days in busy environments.
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCleanupOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="secondary"
              disabled={cleanupMutation.isPending || cleanupDays < 1}
              onClick={() => cleanupMutation.mutate(cleanupDays)}
            >
              {cleanupMutation.isPending ? (
                <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              ) : (
                <Trash2 className="w-4 h-4 mr-2" />
              )}
              Cleanup
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={!!detailsJobId}
        onOpenChange={(open) => (!open ? setDetailsJobId(null) : null)}
      >
        <DialogContent className="sm:max-w-4xl max-h-[90vh] flex flex-col">
          <DialogHeader className="space-y-1">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div className="min-w-0 flex-1">
                <DialogTitle>Job details</DialogTitle>
                <DialogDescription className="font-mono text-xs break-all mt-1">
                  {detailsJobId ?? ""}
                </DialogDescription>
              </div>
              {jobDetailsQuery.data && (
                <div className="flex items-center gap-2 shrink-0">
                  {getStatusBadge(jobDetailsQuery.data.status)}
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="outline" size="sm">
                        <MoreVertical className="w-4 h-4 mr-2" />
                        Actions
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="w-48">
                      <DropdownMenuItem
                        onClick={() =>
                          queryClient.invalidateQueries({
                            queryKey: queryKeys.job(jobDetailsQuery.data!.id),
                          })
                        }
                      >
                        <RefreshCw className="mr-2 h-4 w-4" />
                        Refresh details
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={
                          isTerminalStatus(jobDetailsQuery.data.status) ||
                          jobDetailsQuery.data.cancelRequested
                        }
                        onClick={() =>
                          cancelMutation.mutate(jobDetailsQuery.data!.id)
                        }
                      >
                        <XCircle className="mr-2 h-4 w-4" />
                        Cancel job
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={
                          deleteMutation.isPending ||
                          !(
                            isTerminalStatus(jobDetailsQuery.data.status) ||
                            jobDetailsQuery.data.cancelRequested
                          )
                        }
                        onClick={() =>
                          deleteMutation.mutate(jobDetailsQuery.data!.id)
                        }
                      >
                        <Trash2 className="mr-2 h-4 w-4" />
                        Delete job
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              )}
            </div>
          </DialogHeader>

          <div className="flex-1 overflow-y-auto -mx-6 px-6">
            {jobDetailsQuery.isPending ? (
              <div className="py-8">
                <LoadingArea />
              </div>
            ) : jobDetailsQuery.isError ? (
              <ErrorArea error={jobDetailsQuery.error} />
            ) : jobDetailsQuery.data ? (
              <div className="space-y-6 pt-2">
                <div className="rounded-md border p-4 space-y-4">
                  <div className="flex flex-wrap items-baseline gap-x-6 gap-y-1 text-xs">
                    <span>
                      <span className="text-muted-foreground">Type</span>{" "}
                      <span className="font-medium">
                        {jobDetailsQuery.data.jobType}
                      </span>
                    </span>
                    <span>
                      <span className="text-muted-foreground">Priority</span>{" "}
                      <span className="font-mono">
                        {jobDetailsQuery.data.priority}
                      </span>
                    </span>
                    <span>
                      <span className="text-muted-foreground">Retries</span>{" "}
                      <span className="font-mono">
                        {jobDetailsQuery.data.retryCount}
                      </span>
                    </span>
                    {jobDetailsQuery.data.workerId && (
                      <span>
                        <span className="text-muted-foreground">Worker</span>{" "}
                        <span className="font-mono">
                          {jobDetailsQuery.data.workerId}
                        </span>
                      </span>
                    )}
                  </div>

                  <div className="grid grid-cols-2 sm:grid-cols-4 gap-x-6 gap-y-2 text-xs">
                    <div>
                      <div className="text-muted-foreground uppercase tracking-wider">
                        Created
                      </div>
                      <div
                        className="font-mono"
                        title={formatDateTimeFull(
                          jobDetailsQuery.data.createdAt
                        )}
                      >
                        {formatDateTime(jobDetailsQuery.data.createdAt)}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground uppercase tracking-wider">
                        Scheduled
                      </div>
                      <div
                        className="font-mono"
                        title={formatDateTimeFull(
                          jobDetailsQuery.data.scheduledAt
                        )}
                      >
                        {formatDateTime(jobDetailsQuery.data.scheduledAt)}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground uppercase tracking-wider">
                        Started
                      </div>
                      <div
                        className="font-mono"
                        title={formatDateTimeFull(
                          jobDetailsQuery.data.startedAt
                        )}
                      >
                        {formatDateTime(jobDetailsQuery.data.startedAt)}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground uppercase tracking-wider">
                        Completed
                      </div>
                      <div
                        className="font-mono"
                        title={formatDateTimeFull(
                          jobDetailsQuery.data.completedAt
                        )}
                      >
                        {formatDateTime(jobDetailsQuery.data.completedAt)}
                      </div>
                    </div>
                  </div>

                  {typeof progressPercent(jobDetailsQuery.data) === "number" && (
                    <div className="space-y-1">
                      <div className="flex items-center justify-between text-xs text-muted-foreground">
                        <span>
                          {jobDetailsQuery.data.processedItems}
                          {jobDetailsQuery.data.totalItems
                            ? ` / ${jobDetailsQuery.data.totalItems}`
                            : ""}
                        </span>
                        <span className="font-mono">
                          {progressPercent(jobDetailsQuery.data)!.toFixed(1)}%
                        </span>
                      </div>
                      <ProgressBar
                        value={progressPercent(jobDetailsQuery.data)!}
                      />
                    </div>
                  )}

                  {jobDetailsQuery.data.cancelRequested && (
                    <div className="text-xs text-warning flex items-center gap-1">
                      <AlertTriangle className="w-3 h-3" />
                      Cancel requested
                    </div>
                  )}

                  {jobDetailsQuery.data.errorMessage && (
                    <div className="rounded border border-destructive/30 bg-destructive/5 p-3 text-sm">
                      <div className="font-medium text-destructive">Error</div>
                      <div
                        className="text-xs text-muted-foreground mt-1"
                        title={formatDateTimeFull(
                          jobDetailsQuery.data.lastErrorAt
                        )}
                      >
                        {formatDateTime(jobDetailsQuery.data.lastErrorAt)}
                      </div>
                      <div className="mt-2 whitespace-pre-wrap wrap-break-word text-xs">
                        {jobDetailsQuery.data.errorMessage}
                      </div>
                    </div>
                  )}
                </div>

                <div>
                  <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-3">
                    Parameters
                  </div>
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    <div className="rounded-md border bg-muted/30 overflow-hidden">
                      <div className="px-3 py-2 border-b text-xs font-medium text-muted-foreground">
                        parameters
                      </div>
                      <JsonViewer
                        data={jobDetailsQuery.data.parameters}
                        className="rounded-none"
                      />
                    </div>
                    <div className="rounded-md border bg-muted/30 overflow-hidden">
                      <div className="px-3 py-2 border-b text-xs font-medium text-muted-foreground">
                        retryPolicy
                      </div>
                      <JsonViewer
                        data={jobDetailsQuery.data.retryPolicy}
                        className="rounded-none"
                      />
                    </div>
                  </div>
                </div>
              </div>
            ) : null}
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default JobsDisplay;
