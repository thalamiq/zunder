import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
import {
  RefreshCw,
  CheckCircle2,
  XCircle,
  AlertCircle,
  Loader2,
} from "lucide-react";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import { listTransactions, getTransaction } from "@/api/transactions";
import type { TransactionListItem } from "@/api/transactions";
import { queryKeys } from "@/api/query-keys";
import { formatDateTime, formatDateTimeFull, formatNumber } from "@/lib/utils";
import { PageHeader } from "./PageHeader";

const getStatusBadge = (status: string) => {
  switch (status) {
    case "completed":
      return (
        <Badge className="bg-success">
          <CheckCircle2 className="h-3 w-3 mr-1" />
          Completed
        </Badge>
      );
    case "failed":
      return (
        <Badge variant="destructive">
          <XCircle className="h-3 w-3 mr-1" />
          Failed
        </Badge>
      );
    case "partial":
      return (
        <Badge className="bg-amber-500 hover:bg-amber-600">
          <AlertCircle className="h-3 w-3 mr-1" />
          Partial
        </Badge>
      );
    case "processing":
      return (
        <Badge className="bg-blue-500 hover:bg-blue-600">
          <Loader2 className="h-3 w-3 mr-1 animate-spin" />
          Processing
        </Badge>
      );
    default:
      return <Badge variant="outline">{status}</Badge>;
  }
};

const getTypeBadge = (type: string) => {
  switch (type) {
    case "batch":
      return <Badge className="bg-purple-500 hover:bg-purple-600">Batch</Badge>;
    case "transaction":
      return <Badge className="bg-blue-500 hover:bg-blue-600">Transaction</Badge>;
    default:
      return <Badge variant="outline">{type}</Badge>;
  }
};

const getHttpMethodBadge = (method: string) => {
  const m = method.toUpperCase();
  const variants: Record<string, string> = {
    GET: "bg-blue-500 hover:bg-blue-600",
    POST: "bg-green-500 hover:bg-green-600",
    PUT: "bg-amber-500 hover:bg-amber-600",
    PATCH: "bg-purple-500 hover:bg-purple-600",
    DELETE: "bg-red-500 hover:bg-red-600",
  };
  if (!m) return null;
  return (
    <Badge className={variants[m] || "bg-gray-500 hover:bg-gray-600"}>
      {m}
    </Badge>
  );
};

const formatDuration = (
  startedAt: string | null,
  completedAt: string | null,
): string => {
  if (!startedAt || !completedAt) return "-";
  const start = new Date(startedAt).getTime();
  const end = new Date(completedAt).getTime();
  const ms = end - start;
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
};

const TransactionDisplay = () => {
  const queryClient = useQueryClient();

  const [limit, setLimit] = useState(50);
  const [offset, setOffset] = useState(0);
  const [typeFilter, setTypeFilter] = useState<string | undefined>(undefined);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(
    undefined,
  );
  const [search, setSearch] = useState("");
  const [detailsId, setDetailsId] = useState<string | null>(null);

  const listQuery = useQuery({
    queryKey: queryKeys.transactions(typeFilter, statusFilter, limit, offset),
    queryFn: () =>
      listTransactions({
        bundleType: typeFilter,
        status: statusFilter,
        limit,
        offset,
      }),
    refetchInterval: 30000,
  });

  const detailQuery = useQuery({
    queryKey: detailsId !== null ? queryKeys.transaction(detailsId) : ["transaction", "none"],
    queryFn: () => {
      if (detailsId === null) {
        throw new Error("No transaction selected");
      }
      return getTransaction(detailsId);
    },
    enabled: detailsId !== null,
  });

  const allItems = useMemo(
    () => listQuery.data?.items ?? [],
    [listQuery.data?.items],
  );
  const total = listQuery.data?.total ?? 0;

  const filteredItems = useMemo(() => {
    if (!search.trim()) return allItems;
    const needle = search.trim().toLowerCase();
    return allItems.filter((item: TransactionListItem) => {
      return (
        item.id.toLowerCase().includes(needle) ||
        item.type.toLowerCase().includes(needle) ||
        item.status.toLowerCase().includes(needle) ||
        (item.errorMessage ?? "").toLowerCase().includes(needle)
      );
    });
  }, [allItems, search]);

  const stats = useMemo(() => {
    const counts = allItems.reduce(
      (acc: Record<string, number>, item: TransactionListItem) => {
        acc[item.type] = (acc[item.type] ?? 0) + 1;
        acc[item.status] = (acc[item.status] ?? 0) + 1;
        return acc;
      },
      {} as Record<string, number>,
    );
    return {
      batch: counts["batch"] ?? 0,
      transaction: counts["transaction"] ?? 0,
      completed: counts["completed"] ?? 0,
      failed: counts["failed"] ?? 0,
    };
  }, [allItems]);

  const currentPage = Math.floor(offset / limit) + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));

  if (listQuery.isPending) {
    return <LoadingArea />;
  }

  if (listQuery.isError) {
    return <ErrorArea error={listQuery.error} />;
  }

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Transactions"
        description="View batch and transaction bundle operations"
      />
      <Card>
        <CardHeader>
          <div className="flex items-center justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => {
                  queryClient.invalidateQueries({
                    queryKey: ["transactions"],
                  });
                  if (detailsId !== null) {
                    queryClient.invalidateQueries({
                      queryKey: queryKeys.transaction(detailsId),
                    });
                  }
                }}
              >
                <RefreshCw className="w-4 h-4 mr-2" />
                Refresh
              </Button>
          </div>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Total (all)
              </div>
              <div className="text-2xl font-bold">{formatNumber(total)}</div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Batch
              </div>
              <div className="text-2xl font-bold text-purple-500">
                {formatNumber(stats.batch)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Transaction
              </div>
              <div className="text-2xl font-bold text-blue-500">
                {formatNumber(stats.transaction)}
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
          </div>
          <div className="mt-4 text-sm text-muted-foreground">
            Showing {formatNumber(allItems.length)} of {formatNumber(total)}{" "}
            transactions (page {currentPage} of {totalPages})
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Transaction List</CardTitle>
          <CardDescription>
            Filter and inspect batch/transaction operations
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col lg:flex-row gap-3">
            <div className="flex-1">
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search by id, type, status, error..."
              />
            </div>
            <div className="flex gap-2 flex-wrap items-center">
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Type</span>
                <Select
                  value={typeFilter ?? "__all__"}
                  onValueChange={(value) => {
                    const next = value === "__all__" ? undefined : value;
                    setTypeFilter(next);
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[140px]">
                    <SelectValue placeholder="All" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All</SelectItem>
                    <SelectItem value="batch">Batch</SelectItem>
                    <SelectItem value="transaction">Transaction</SelectItem>
                  </SelectContent>
                </Select>
              </div>
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
                    <SelectItem value="processing">Processing</SelectItem>
                    <SelectItem value="completed">Completed</SelectItem>
                    <SelectItem value="failed">Failed</SelectItem>
                    <SelectItem value="partial">Partial</SelectItem>
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

          {filteredItems.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-10">
              No transactions match your filters.
            </div>
          ) : (
            <div className="rounded-md border overflow-hidden">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Status</TableHead>
                    <TableHead>Type</TableHead>
                    <TableHead>Entries</TableHead>
                    <TableHead>Created</TableHead>
                    <TableHead>Duration</TableHead>
                    <TableHead>Error</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredItems.map((item: TransactionListItem) => (
                    <TableRow
                      key={item.id}
                      className="hover:bg-accent/30 cursor-pointer"
                      onClick={() => setDetailsId(item.id)}
                    >
                      <TableCell>{getStatusBadge(item.status)}</TableCell>
                      <TableCell>{getTypeBadge(item.type)}</TableCell>
                      <TableCell>
                        <span className="font-mono">
                          {item.entryCount ?? 0}
                        </span>
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        <span title={formatDateTimeFull(item.createdAt)}>
                          {formatDateTime(item.createdAt)}
                        </span>
                      </TableCell>
                      <TableCell className="text-xs font-mono">
                        {formatDuration(item.startedAt, item.completedAt)}
                      </TableCell>
                      <TableCell>
                        {item.errorMessage ? (
                          <span
                            className="text-xs text-destructive truncate max-w-[200px] block"
                            title={item.errorMessage}
                          >
                            {item.errorMessage}
                          </span>
                        ) : (
                          <span className="text-muted-foreground">-</span>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}

          <div className="flex items-center justify-between pt-2">
            <div className="text-xs text-muted-foreground">
              Offset {formatNumber(offset)} - Limit {formatNumber(limit)}
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

      <Dialog
        open={detailsId !== null}
        onOpenChange={(open) => (!open ? setDetailsId(null) : null)}
      >
        <DialogContent className="sm:max-w-4xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Transaction Details</DialogTitle>
            <DialogDescription className="font-mono text-xs">
              ID: {detailsId ?? ""}
            </DialogDescription>
          </DialogHeader>

          {detailQuery.isPending ? (
            <div className="py-8">
              <LoadingArea />
            </div>
          ) : detailQuery.isError ? (
            <ErrorArea error={detailQuery.error} />
          ) : detailQuery.data ? (
            <div className="space-y-4">
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg">Overview</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-3">
                    <div className="grid grid-cols-2 gap-3 text-sm">
                      <div>
                        <div className="text-muted-foreground">Type</div>
                        <div>{getTypeBadge(detailQuery.data.type)}</div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Status</div>
                        <div>{getStatusBadge(detailQuery.data.status)}</div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Entries</div>
                        <div className="font-mono">
                          {detailQuery.data.entryCount ?? 0}
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Duration</div>
                        <div className="font-mono">
                          {formatDuration(
                            detailQuery.data.startedAt,
                            detailQuery.data.completedAt,
                          )}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>

                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg">Timing</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-3">
                    <div className="grid grid-cols-1 gap-3 text-sm">
                      <div>
                        <div className="text-muted-foreground">Created</div>
                        <div
                          title={formatDateTimeFull(
                            detailQuery.data.createdAt,
                          )}
                        >
                          {formatDateTime(detailQuery.data.createdAt)}
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Started</div>
                        <div>
                          {detailQuery.data.startedAt
                            ? formatDateTime(detailQuery.data.startedAt)
                            : "-"}
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Completed</div>
                        <div>
                          {detailQuery.data.completedAt
                            ? formatDateTime(detailQuery.data.completedAt)
                            : "-"}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              </div>

              {detailQuery.data.errorMessage && (
                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg text-destructive">
                      Error
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <pre className="text-sm text-destructive whitespace-pre-wrap break-all">
                      {detailQuery.data.errorMessage}
                    </pre>
                  </CardContent>
                </Card>
              )}

              {detailQuery.data.entries.length > 0 && (
                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg">Entries</CardTitle>
                    <CardDescription>
                      {detailQuery.data.entries.length} entries in this bundle
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <div className="rounded-md border overflow-hidden">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead className="w-[60px]">#</TableHead>
                            <TableHead>Method</TableHead>
                            <TableHead>URL</TableHead>
                            <TableHead>Status</TableHead>
                            <TableHead>Resource</TableHead>
                            <TableHead>Error</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {detailQuery.data.entries.map((entry) => (
                            <TableRow key={entry.entryIndex}>
                              <TableCell className="font-mono text-xs">
                                {entry.entryIndex}
                              </TableCell>
                              <TableCell>
                                {getHttpMethodBadge(entry.method)}
                              </TableCell>
                              <TableCell className="text-xs font-mono max-w-[200px] truncate">
                                <span title={entry.url}>{entry.url || "-"}</span>
                              </TableCell>
                              <TableCell>
                                {entry.status !== null ? (
                                  <Badge
                                    variant={
                                      entry.status >= 400
                                        ? "destructive"
                                        : "secondary"
                                    }
                                  >
                                    {entry.status}
                                  </Badge>
                                ) : (
                                  <span className="text-muted-foreground">
                                    -
                                  </span>
                                )}
                              </TableCell>
                              <TableCell>
                                {entry.resourceType ? (
                                  <div className="space-y-1">
                                    <div className="font-medium text-xs">
                                      {entry.resourceType}
                                    </div>
                                    {entry.resourceId && (
                                      <div className="text-xs text-muted-foreground font-mono">
                                        {entry.resourceId}
                                      </div>
                                    )}
                                  </div>
                                ) : (
                                  <span className="text-muted-foreground">
                                    -
                                  </span>
                                )}
                              </TableCell>
                              <TableCell>
                                {entry.errorMessage ? (
                                  <span
                                    className="text-xs text-destructive truncate max-w-[150px] block"
                                    title={entry.errorMessage}
                                  >
                                    {entry.errorMessage}
                                  </span>
                                ) : (
                                  <span className="text-muted-foreground">
                                    -
                                  </span>
                                )}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                  </CardContent>
                </Card>
              )}
            </div>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default TransactionDisplay;
