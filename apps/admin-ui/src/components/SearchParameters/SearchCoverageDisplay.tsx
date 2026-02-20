import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { formatNumber, formatDate } from "@/lib/utils";
import { Progress } from "@thalamiq/ui/components/progress";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableCell,
  TableHead,
} from "@thalamiq/ui/components/table";
import { Input } from "@thalamiq/ui/components/input";
import { Button } from "@thalamiq/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@thalamiq/ui/components/dialog";
import { CheckCircle2, AlertTriangle } from "lucide-react";
import { queryKeys } from "@/api/query-keys";
import {
  getSearchParameterIndexingStatus,
  SearchParameterIndexingStatusRecord,
} from "@/api/search";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import JsonViewer from "@/components/JsonViewer";
import { PageHeader } from "../PageHeader";

const percent = (p: number): number => Math.max(0, Math.min(100, p));

const statusBadge = (indexingNeeded: boolean) => {
  if (!indexingNeeded) {
    return (
      <Badge className="bg-success">
        <CheckCircle2 className="w-3 h-3 mr-1" />
        Up to date
      </Badge>
    );
  }
  return (
    <Badge className="bg-warning">
      <AlertTriangle className="w-3 h-3 mr-1" />
      Reindex needed
    </Badge>
  );
};

const SearchCoverageDisplay = () => {
  const indexingStatusQuery = useQuery({
    queryKey: queryKeys.searchParameterIndexingStatus(),
    queryFn: () => getSearchParameterIndexingStatus(),
    refetchInterval: 30000,
  });

  const [resourceTypeFilter, setResourceTypeFilter] = useState("");
  const [onlyNeedsIndexing, setOnlyNeedsIndexing] = useState(false);
  const [selectedCoverage, setSelectedCoverage] =
    useState<SearchParameterIndexingStatusRecord | null>(null);

  const coverageRows = useMemo(
    () => indexingStatusQuery.data ?? [],
    [indexingStatusQuery.data]
  );

  const coverageSummary = useMemo(() => {
    const totalTypes = coverageRows.length;
    const needsIndexing = coverageRows.filter((r) => r.indexingNeeded).length;
    const totalResources = coverageRows.reduce(
      (acc, r) => acc + r.totalResources,
      0
    );
    const indexedCurrent = coverageRows.reduce(
      (acc, r) => acc + r.indexedWithCurrent,
      0
    );
    const indexedOld = coverageRows.reduce(
      (acc, r) => acc + r.indexedWithOld,
      0
    );
    const neverIndexed = coverageRows.reduce(
      (acc, r) => acc + r.neverIndexed,
      0
    );
    const avgCoverage =
      totalTypes === 0
        ? 0
        : coverageRows.reduce((acc, r) => acc + r.coveragePercent, 0) /
          totalTypes;

    return {
      totalTypes,
      needsIndexing,
      totalResources,
      indexedCurrent,
      indexedOld,
      neverIndexed,
      avgCoverage,
    };
  }, [coverageRows]);

  const filteredCoverageRows = useMemo(() => {
    let rows = coverageRows;
    if (resourceTypeFilter.trim()) {
      const needle = resourceTypeFilter.trim().toLowerCase();
      rows = rows.filter((r) => r.resourceType.toLowerCase().includes(needle));
    }
    if (onlyNeedsIndexing) {
      rows = rows.filter((r) => r.indexingNeeded);
    }
    return rows;
  }, [coverageRows, resourceTypeFilter, onlyNeedsIndexing]);

  if (indexingStatusQuery.isPending) {
    return <LoadingArea />;
  }

  if (indexingStatusQuery.isError) {
    return <ErrorArea error={indexingStatusQuery.error} />;
  }

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Coverage"
        description="Indexing coverage per resource type"
      />
      <Card>
        <CardContent className="pt-6">
          <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4">
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Resource types
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(coverageSummary.totalTypes)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Needs reindex
              </div>
              <div className="text-2xl font-bold text-amber-600">
                {formatNumber(coverageSummary.needsIndexing)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Total resources
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(coverageSummary.totalResources)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Indexed (current)
              </div>
              <div className="text-2xl font-bold text-success">
                {formatNumber(coverageSummary.indexedCurrent)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Indexed (old)
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(coverageSummary.indexedOld)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Never indexed
              </div>
              <div className="text-2xl font-bold text-destructive">
                {formatNumber(coverageSummary.neverIndexed)}
              </div>
            </div>
          </div>
          <div className="mt-4">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>Average coverage</span>
              <span className="font-mono">
                {coverageSummary.avgCoverage.toFixed(2)}%
              </span>
            </div>
            <Progress value={coverageSummary.avgCoverage} />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Resource Type Coverage</CardTitle>
          <CardDescription>
            Shows when manual reindexing is necessary
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col lg:flex-row gap-3">
            <div className="flex-1">
              <Input
                value={resourceTypeFilter}
                onChange={(e) => setResourceTypeFilter(e.target.value)}
                placeholder="Filter by resource type..."
              />
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant={onlyNeedsIndexing ? "secondary" : "outline"}
                onClick={() => setOnlyNeedsIndexing((v) => !v)}
              >
                {onlyNeedsIndexing
                  ? "Showing: needs reindex"
                  : "Filter: needs reindex"}
              </Button>
            </div>
          </div>

          {filteredCoverageRows.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-10">
              No resource types match your filters.
            </div>
          ) : (
            <div className="rounded-md border overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Resource Type</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Coverage</TableHead>
                    <TableHead className="text-right">Indexed (old)</TableHead>
                    <TableHead className="text-right">Never indexed</TableHead>
                    <TableHead className="text-right">Search params</TableHead>
                    <TableHead>Last change</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredCoverageRows.map((row) => (
                    <TableRow
                      key={row.resourceType}
                      className="hover:bg-accent/30 cursor-pointer"
                      onClick={() => setSelectedCoverage(row)}
                    >
                      <TableCell className="font-medium">
                        {row.resourceType}
                      </TableCell>
                      <TableCell>{statusBadge(row.indexingNeeded)}</TableCell>
                      <TableCell>
                        <div className="space-y-1 min-w-[220px]">
                          <div className="flex items-center justify-between text-xs text-muted-foreground">
                            <span>
                              {formatNumber(row.indexedWithCurrent)} /{" "}
                              {formatNumber(row.totalResources)}
                            </span>
                            <span className="font-mono">
                              {percent(row.coveragePercent).toFixed(2)}%
                            </span>
                          </div>
                          <Progress value={row.coveragePercent} />
                        </div>
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        {formatNumber(row.indexedWithOld)}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        {formatNumber(row.neverIndexed)}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        {formatNumber(row.paramCount)}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {formatDate(row.lastParameterChange)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
          <div className="text-xs text-muted-foreground">
            When SearchParameters change, existing resources may remain indexed
            with an old hash until reindexed.
          </div>
        </CardContent>
      </Card>

      <Dialog
        open={!!selectedCoverage}
        onOpenChange={(o) => (!o ? setSelectedCoverage(null) : null)}
      >
        <DialogContent className="sm:min-w-4xl min-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Coverage details</DialogTitle>
            <DialogDescription>
              {selectedCoverage?.resourceType
                ? `Resource type: ${selectedCoverage.resourceType}`
                : ""}
            </DialogDescription>
          </DialogHeader>
          {selectedCoverage && (
            <div className="space-y-4">
              <div className="grid grid-cols-1 md:grid-cols-3 gap-3 text-sm">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Index status</CardTitle>
                    <CardDescription>Current vs old hashes</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">
                        Indexed (current)
                      </span>
                      <span className="font-mono">
                        {formatNumber(selectedCoverage.indexedWithCurrent)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">
                        Indexed (old)
                      </span>
                      <span className="font-mono">
                        {formatNumber(selectedCoverage.indexedWithOld)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">
                        Never indexed
                      </span>
                      <span className="font-mono">
                        {formatNumber(selectedCoverage.neverIndexed)}
                      </span>
                    </div>
                    <div className="pt-2">
                      {statusBadge(selectedCoverage.indexingNeeded)}
                    </div>
                  </CardContent>
                </Card>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">
                      Search parameters
                    </CardTitle>
                    <CardDescription>Versioned hash</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Version</span>
                      <span className="font-mono">
                        {selectedCoverage.versionNumber}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Param count</span>
                      <span className="font-mono">
                        {selectedCoverage.paramCount}
                      </span>
                    </div>
                    <div className="text-muted-foreground">Hash</div>
                    <div className="font-mono text-xs break-all">
                      {selectedCoverage.currentHash}
                    </div>
                    <div className="text-muted-foreground pt-2">
                      Last change
                    </div>
                    <div>
                      {formatDate(selectedCoverage.lastParameterChange)}
                    </div>
                  </CardContent>
                </Card>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Indexed at</CardTitle>
                    <CardDescription>Range</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="text-muted-foreground">Oldest</div>
                    <div>{formatDate(selectedCoverage.oldestIndexedAt)}</div>
                    <div className="text-muted-foreground pt-2">Newest</div>
                    <div>{formatDate(selectedCoverage.newestIndexedAt)}</div>
                  </CardContent>
                </Card>
              </div>
              <div className="rounded-md border">
                <div className="px-4 py-2 border-b text-sm font-medium">
                  Raw
                </div>
                <JsonViewer data={selectedCoverage} />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setSelectedCoverage(null)}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default SearchCoverageDisplay;
