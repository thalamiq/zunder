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
  RefreshCw,
  CheckCircle2,
  XCircle,
  AlertCircle,
  FileText,
  Eye,
  Pencil,
  Trash2,
  Search,
} from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import JsonViewer from "@/components/JsonViewer";
import { listAuditEvents, getAuditEvent } from "@/api/audit";
import { queryKeys } from "@/api/query-keys";
import { formatDateTime, formatDateTimeFull, formatNumber } from "@/lib/utils";
import { PageHeader } from "./PageHeader";

const getOutcomeBadge = (outcome: string) => {
  const o = outcome.toLowerCase();

  if (o === "success" || o === "0") {
    return (
      <Badge className="bg-success">
        <CheckCircle2 className="h-3 w-3 mr-1" />
        Success
      </Badge>
    );
  }
  if (o === "minor-failure" || o === "4") {
    return (
      <Badge className="bg-amber-500 hover:bg-amber-600">
        <AlertCircle className="h-3 w-3 mr-1" />
        Minor Failure
      </Badge>
    );
  }
  if (o === "serious-failure" || o === "8") {
    return (
      <Badge variant="destructive">
        <XCircle className="h-3 w-3 mr-1" />
        Serious Failure
      </Badge>
    );
  }
  if (o === "major-failure" || o === "12") {
    return (
      <Badge variant="destructive">
        <XCircle className="h-3 w-3 mr-1" />
        Major Failure
      </Badge>
    );
  }
  return <Badge variant="outline">{outcome}</Badge>;
};

const getActionIcon = (action: string) => {
  const a = action.toLowerCase();
  if (a === "r" || a === "read") {
    return <Eye className="h-4 w-4" />;
  }
  if (a === "c" || a === "create") {
    return <FileText className="h-4 w-4" />;
  }
  if (a === "u" || a === "update") {
    return <Pencil className="h-4 w-4" />;
  }
  if (a === "d" || a === "delete") {
    return <Trash2 className="h-4 w-4" />;
  }
  if (a === "e" || a === "execute") {
    return <Search className="h-4 w-4" />;
  }
  return null;
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
  return (
    <Badge className={variants[m] || "bg-gray-500 hover:bg-gray-600"}>
      {m}
    </Badge>
  );
};

const AuditEventDisplay = () => {
  const queryClient = useQueryClient();

  const [limit, setLimit] = useState(50);
  const [offset, setOffset] = useState(0);
  const [actionFilter, setActionFilter] = useState<string | undefined>(undefined);
  const [outcomeFilter, setOutcomeFilter] = useState<string | undefined>(undefined);
  const [resourceTypeFilter, setResourceTypeFilter] = useState<string | undefined>(undefined);
  const [search, setSearch] = useState("");
  const [detailsEventId, setDetailsEventId] = useState<number | null>(null);

  const eventsQuery = useQuery({
    queryKey: queryKeys.auditEvents(
      actionFilter,
      outcomeFilter,
      resourceTypeFilter,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      limit,
      offset
    ),
    queryFn: () =>
      listAuditEvents({
        action: actionFilter,
        outcome: outcomeFilter,
        resourceType: resourceTypeFilter,
        limit,
        offset,
      }),
    refetchInterval: 30000,
  });

  const eventDetailsQuery = useQuery({
    queryKey: detailsEventId !== null ? queryKeys.auditEvent(detailsEventId) : ["auditEvent", "none"],
    queryFn: () => {
      if (detailsEventId === null) {
        throw new Error("No event selected");
      }
      return getAuditEvent(detailsEventId);
    },
    enabled: detailsEventId !== null,
  });

  const allEvents = useMemo(
    () => eventsQuery.data?.items ?? [],
    [eventsQuery.data?.items]
  );
  const total = eventsQuery.data?.total ?? 0;

  const uniqueActions = useMemo(() => {
    const actions = new Set(allEvents.map((e) => e.action));
    return Array.from(actions).sort((a, b) => a.localeCompare(b));
  }, [allEvents]);

  const uniqueOutcomes = useMemo(() => {
    const outcomes = new Set(allEvents.map((e) => e.outcome));
    return Array.from(outcomes).sort((a, b) => a.localeCompare(b));
  }, [allEvents]);

  const uniqueResourceTypes = useMemo(() => {
    const types = new Set(
      allEvents.filter((e) => e.resourceType).map((e) => e.resourceType as string)
    );
    return Array.from(types).sort((a, b) => a.localeCompare(b));
  }, [allEvents]);

  const filteredEvents = useMemo(() => {
    if (!search.trim()) return allEvents;
    const needle = search.trim().toLowerCase();
    return allEvents.filter((event) => {
      return (
        String(event.id).includes(needle) ||
        event.action.toLowerCase().includes(needle) ||
        event.httpMethod.toLowerCase().includes(needle) ||
        event.fhirAction.toLowerCase().includes(needle) ||
        (event.resourceType ?? "").toLowerCase().includes(needle) ||
        (event.resourceId ?? "").toLowerCase().includes(needle) ||
        (event.patientId ?? "").toLowerCase().includes(needle) ||
        (event.requestId ?? "").toLowerCase().includes(needle) ||
        event.outcome.toLowerCase().includes(needle)
      );
    });
  }, [allEvents, search]);

  const stats = useMemo(() => {
    const counts = filteredEvents.reduce((acc, event) => {
      const o = event.outcome.toLowerCase();
      if (o === "success" || o === "0") {
        acc.success = (acc.success ?? 0) + 1;
      } else {
        acc.failure = (acc.failure ?? 0) + 1;
      }
      return acc;
    }, {} as Record<string, number>);
    return {
      success: counts.success ?? 0,
      failure: counts.failure ?? 0,
      total: filteredEvents.length,
    };
  }, [filteredEvents]);

  const currentPage = Math.floor(offset / limit) + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));

  if (eventsQuery.isPending) {
    return <LoadingArea />;
  }

  if (eventsQuery.isError) {
    return <ErrorArea error={eventsQuery.error} />;
  }

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Logs"
        description="View and inspect audit events for FHIR operations"
      />
      <Card>
        <CardHeader>
          <div className="flex items-center justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => {
                  queryClient.invalidateQueries({ queryKey: ["auditEvents"] });
                  if (detailsEventId !== null) {
                    queryClient.invalidateQueries({
                      queryKey: queryKeys.auditEvent(detailsEventId),
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
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
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
                Success
              </div>
              <div className="text-2xl font-bold text-success">
                {formatNumber(stats.success)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Failures
              </div>
              <div className="text-2xl font-bold text-destructive">
                {formatNumber(stats.failure)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Total (all)
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(total)}
              </div>
            </div>
          </div>
          <div className="mt-4 text-sm text-muted-foreground">
            Showing {formatNumber(allEvents.length)} of {formatNumber(total)} events
            (page {currentPage} of {totalPages})
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Event List</CardTitle>
          <CardDescription>Filter and inspect audit events</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col lg:flex-row gap-3">
            <div className="flex-1">
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search by id, action, resource type, patient, request id..."
              />
            </div>
            <div className="flex gap-2 flex-wrap items-center">
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Action</span>
                <Select
                  value={actionFilter ?? "__all__"}
                  onValueChange={(value) => {
                    const next = value === "__all__" ? undefined : value;
                    setActionFilter(next);
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[120px]">
                    <SelectValue placeholder="All" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All</SelectItem>
                    {uniqueActions.map((a) => (
                      <SelectItem key={a} value={a}>
                        {a}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Outcome</span>
                <Select
                  value={outcomeFilter ?? "__all__"}
                  onValueChange={(value) => {
                    const next = value === "__all__" ? undefined : value;
                    setOutcomeFilter(next);
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[140px]">
                    <SelectValue placeholder="All" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All</SelectItem>
                    {uniqueOutcomes.map((o) => (
                      <SelectItem key={o} value={o}>
                        {o}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Resource</span>
                <Select
                  value={resourceTypeFilter ?? "__all__"}
                  onValueChange={(value) => {
                    const next = value === "__all__" ? undefined : value;
                    setResourceTypeFilter(next);
                    setOffset(0);
                  }}
                >
                  <SelectTrigger className="h-9 w-[140px]">
                    <SelectValue placeholder="All" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All</SelectItem>
                    {uniqueResourceTypes.map((t) => (
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

          {filteredEvents.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-10">
              No audit events match your filters.
            </div>
          ) : (
            <div className="rounded-md border overflow-hidden">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Timestamp</TableHead>
                    <TableHead>Action</TableHead>
                    <TableHead>Method</TableHead>
                    <TableHead>Resource</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Outcome</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredEvents.map((event) => (
                    <TableRow
                      key={event.id}
                      className="hover:bg-accent/30 cursor-pointer"
                      onClick={() => setDetailsEventId(event.id)}
                    >
                      <TableCell className="text-xs text-muted-foreground">
                        <span title={formatDateTimeFull(event.timestamp)}>
                          {formatDateTime(event.timestamp)}
                        </span>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          {getActionIcon(event.action)}
                          <span className="font-medium">{event.fhirAction}</span>
                        </div>
                      </TableCell>
                      <TableCell>
                        {getHttpMethodBadge(event.httpMethod)}
                      </TableCell>
                      <TableCell>
                        <div className="space-y-1">
                          {event.resourceType ? (
                            <>
                              <div className="font-medium">{event.resourceType}</div>
                              {event.resourceId && (
                                <div className="text-xs text-muted-foreground font-mono">
                                  {event.resourceId}
                                </div>
                              )}
                            </>
                          ) : (
                            <span className="text-muted-foreground">—</span>
                          )}
                        </div>
                      </TableCell>
                      <TableCell>
                        <Badge variant={event.statusCode >= 400 ? "destructive" : "secondary"}>
                          {event.statusCode}
                        </Badge>
                      </TableCell>
                      <TableCell>{getOutcomeBadge(event.outcome)}</TableCell>
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
        open={detailsEventId !== null}
        onOpenChange={(open) => (!open ? setDetailsEventId(null) : null)}
      >
        <DialogContent className="sm:max-w-4xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Audit Event Details</DialogTitle>
            <DialogDescription className="font-mono text-xs">
              ID: {detailsEventId ?? ""}
            </DialogDescription>
          </DialogHeader>

          {eventDetailsQuery.isPending ? (
            <div className="py-8">
              <LoadingArea />
            </div>
          ) : eventDetailsQuery.isError ? (
            <ErrorArea error={eventDetailsQuery.error} />
          ) : eventDetailsQuery.data ? (
            <div className="space-y-4">
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg">Request Info</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-3">
                    <div className="grid grid-cols-2 gap-3 text-sm">
                      <div>
                        <div className="text-muted-foreground">Timestamp</div>
                        <div title={formatDateTimeFull(eventDetailsQuery.data.timestamp)}>
                          {formatDateTime(eventDetailsQuery.data.timestamp)}
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">HTTP Method</div>
                        <div>{getHttpMethodBadge(eventDetailsQuery.data.httpMethod)}</div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Action</div>
                        <div className="flex items-center gap-2">
                          {getActionIcon(eventDetailsQuery.data.action)}
                          <span>{eventDetailsQuery.data.fhirAction}</span>
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Status Code</div>
                        <Badge variant={eventDetailsQuery.data.statusCode >= 400 ? "destructive" : "secondary"}>
                          {eventDetailsQuery.data.statusCode}
                        </Badge>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Outcome</div>
                        <div>{getOutcomeBadge(eventDetailsQuery.data.outcome)}</div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Request ID</div>
                        <div className="font-mono text-xs break-all">
                          {eventDetailsQuery.data.requestId || "—"}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>

                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg">Resource Info</CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-3">
                    <div className="grid grid-cols-2 gap-3 text-sm">
                      <div>
                        <div className="text-muted-foreground">Resource Type</div>
                        <div>{eventDetailsQuery.data.resourceType || "—"}</div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Resource ID</div>
                        <div className="font-mono text-xs">
                          {eventDetailsQuery.data.resourceId || "—"}
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Version ID</div>
                        <div className="font-mono">
                          {eventDetailsQuery.data.versionId ?? "—"}
                        </div>
                      </div>
                      <div>
                        <div className="text-muted-foreground">Patient ID</div>
                        <div className="font-mono text-xs">
                          {eventDetailsQuery.data.patientId || "—"}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              </div>

              <Card>
                <CardHeader>
                  <CardTitle className="text-lg">Client Info</CardTitle>
                </CardHeader>
                <CardContent className="space-y-3">
                  <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
                    <div>
                      <div className="text-muted-foreground">Client ID</div>
                      <div className="font-mono text-xs">
                        {eventDetailsQuery.data.clientId || "—"}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">User ID</div>
                      <div className="font-mono text-xs">
                        {eventDetailsQuery.data.userId || "—"}
                      </div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">Token Type</div>
                      <div>{eventDetailsQuery.data.tokenType || "—"}</div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">Client IP</div>
                      <div className="font-mono text-xs">
                        {eventDetailsQuery.data.clientIp || "—"}
                      </div>
                    </div>
                  </div>
                  {eventDetailsQuery.data.userAgent && (
                    <div>
                      <div className="text-muted-foreground text-sm">User Agent</div>
                      <div className="text-xs text-muted-foreground break-all">
                        {eventDetailsQuery.data.userAgent}
                      </div>
                    </div>
                  )}
                  {eventDetailsQuery.data.scopes.length > 0 && (
                    <div>
                      <div className="text-muted-foreground text-sm mb-2">Scopes</div>
                      <div className="flex flex-wrap gap-1">
                        {eventDetailsQuery.data.scopes.map((scope, i) => (
                          <Badge key={i} variant="outline" className="text-xs font-mono">
                            {scope}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle className="text-lg">FHIR AuditEvent</CardTitle>
                  <CardDescription>
                    Full FHIR AuditEvent resource
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <div className="rounded-md border bg-background">
                    <JsonViewer
                      data={eventDetailsQuery.data.auditEvent}
                      className="rounded-md"
                    />
                  </div>
                </CardContent>
              </Card>

              {eventDetailsQuery.data.details != null && (
                <Card>
                  <CardHeader>
                    <CardTitle className="text-lg">Additional Details</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="rounded-md border bg-background">
                      <JsonViewer
                        data={eventDetailsQuery.data.details}
                        className="rounded-md"
                      />
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

export default AuditEventDisplay;
