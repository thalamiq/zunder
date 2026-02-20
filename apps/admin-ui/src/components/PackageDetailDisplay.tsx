import { useState, useMemo, Fragment } from "react";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { Button } from "@thalamiq/ui/components/button";
import {
  Table,
  TableHead,
  TableRow,
  TableHeader,
  TableBody,
  TableCell,
} from "@thalamiq/ui/components/table";
import SearchInput from "./SearchInput";
import {
  useReactTable,
  getCoreRowModel,
  ColumnDef,
  flexRender,
} from "@tanstack/react-table";
import { PackageRecord, PackageResourceRecord } from "@/api/packages";
import {
  CheckCircle,
  XCircle,
  Clock,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  ChevronRight as ChevronRightIcon,
  AlertTriangle,
} from "lucide-react";
import { Button as UIButton } from "@thalamiq/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@thalamiq/ui/components/dialog";
import { highlightJson } from "@/lib/json";
import { cn } from "@thalamiq/ui/utils";
import { PageHeader } from "./PageHeader";

interface PackageDetailDisplayProps {
  packageData: PackageRecord;
  resourcesData: {
    resources: PackageResourceRecord[];
    total: number;
    limit: number | null;
    offset: number | null;
  };
  onPageChange: (offset: number) => void;
  pageSize?: number;
}

const formatDate = (dateString: string | null | undefined): string => {
  if (!dateString) return "Never";
  try {
    const date = new Date(dateString);
    return date.toLocaleString();
  } catch {
    return dateString;
  }
};

const formatNumber = (num: number): string => {
  return new Intl.NumberFormat().format(num);
};

const getStatusBadge = (status: string) => {
  const statusLower = status.toLowerCase();

  if (statusLower === "completed" || statusLower === "active") {
    return (
      <Badge className="bg-success">
        <CheckCircle className="h-3 w-3 mr-1" />
        {status}
      </Badge>
    );
  }

  if (statusLower === "failed" || statusLower === "error") {
    return (
      <Badge variant="destructive">
        <XCircle className="h-3 w-3 mr-1" />
        {status}
      </Badge>
    );
  }

  if (
    statusLower === "pending" ||
    statusLower === "processing" ||
    statusLower === "installing"
  ) {
    return (
      <Badge variant="secondary">
        <Clock className="h-3 w-3 mr-1" />
        {status}
      </Badge>
    );
  }

  return <Badge variant="outline">{status}</Badge>;
};

export const PackageDetailDisplay = ({
  packageData,
  resourcesData,
  onPageChange,
  pageSize = 50,
}: PackageDetailDisplayProps) => {
  const { resources, total, limit, offset } = resourcesData;
  const [expandedRows, setExpandedRows] = useState<Set<number>>(new Set());
  const [resourceFilter, setResourceFilter] = useState("");

  const currentLimit = limit ?? pageSize;
  const currentOffset = offset ?? 0;
  const currentPage = Math.floor(currentOffset / currentLimit) + 1;
  const totalPages = Math.ceil(total / currentLimit);

  const toggleRow = (index: number) => {
    setExpandedRows((prev) => {
      const next = new Set(prev);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
  };

  // Filter resources
  const filteredResources = useMemo(() => {
    if (!resourceFilter) return resources;
    const filter = resourceFilter.toLowerCase();
    return resources.filter(
      (r) =>
        r.resourceType.toLowerCase().includes(filter) ||
        r.resourceId.toLowerCase().includes(filter)
    );
  }, [resources, resourceFilter]);

  // Table columns
  const columns = useMemo<ColumnDef<PackageResourceRecord>[]>(
    () => [
      {
        id: "expander",
        header: "",
        cell: ({ row }) => {
          const isExpanded = expandedRows.has(row.index);
          return (
            <UIButton
              variant="ghost"
              size="sm"
              className="h-5 w-5 p-0"
              onClick={(e) => {
                e.stopPropagation();
                toggleRow(row.index);
              }}
            >
              {isExpanded ? (
                <ChevronDown className="w-3 h-3" />
              ) : (
                <ChevronRightIcon className="w-3 h-3" />
              )}
            </UIButton>
          );
        },
      },
      {
        accessorKey: "resourceType",
        header: "Resource Type",
        cell: ({ row }) => (
          <span className="font-medium text-sm">
            {row.original.resourceType}
          </span>
        ),
      },
      {
        accessorKey: "resourceId",
        header: "Resource ID",
        cell: ({ row }) => (
          <span className="font-mono text-xs">{row.original.resourceId}</span>
        ),
      },
      {
        accessorKey: "versionId",
        header: "Version",
        cell: ({ row }) => (
          <span className="text-sm">{row.original.versionId}</span>
        ),
      },
      {
        accessorKey: "deleted",
        header: "Status",
        cell: ({ row }) => (
          <Badge variant={row.original.deleted ? "destructive" : "outline"}>
            {row.original.deleted ? "Deleted" : "Active"}
          </Badge>
        ),
      },
      {
        accessorKey: "loadedAt",
        header: "Loaded At",
        cell: ({ row }) => (
          <span className="text-xs text-muted-foreground">
            {formatDate(row.original.loadedAt)}
          </span>
        ),
      },
      {
        accessorKey: "lastUpdated",
        header: "Last Updated",
        cell: ({ row }) => (
          <span className="text-xs text-muted-foreground">
            {formatDate(row.original.lastUpdated)}
          </span>
        ),
      },
    ],
    [expandedRows]
  );

  const table = useReactTable({
    data: filteredResources,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  const handlePrevious = () => {
    if (currentOffset > 0) {
      const newOffset = Math.max(0, currentOffset - currentLimit);
      onPageChange(newOffset);
    }
  };

  const handleNext = () => {
    if (currentOffset + currentLimit < total) {
      const newOffset = currentOffset + currentLimit;
      onPageChange(newOffset);
    }
  };

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title={packageData.name}
        description={`Version: ${packageData.version}`}
      />
      {/* Package Metadata Section */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-end gap-2">
              {getStatusBadge(packageData.status)}
              {(() => {
                const loadSummary =
                  packageData.metadata &&
                  typeof packageData.metadata === "object" &&
                  "load_summary" in packageData.metadata
                    ? (packageData.metadata.load_summary as Record<
                        string,
                        unknown
                      >)
                    : null;
                const sampleFailures =
                  loadSummary &&
                  "sample_failures" in loadSummary &&
                  Array.isArray(loadSummary.sample_failures)
                    ? loadSummary.sample_failures
                    : null;
                const hasFailures =
                  Array.isArray(sampleFailures) && sampleFailures.length > 0;

                if (!hasFailures && !packageData.errorMessage) return null;

                return (
                  <Dialog>
                    <DialogTrigger asChild>
                      <Button variant="destructive" size="sm" className="h-7">
                        {sampleFailures && sampleFailures.length > 0
                          ? sampleFailures.length
                          : "Error"}
                        <AlertTriangle className="h-4 w-4" />
                      </Button>
                    </DialogTrigger>
                    <DialogContent className="max-w-4xl max-h-[85vh] overflow-y-auto">
                      <DialogHeader>
                        <DialogTitle className="flex items-center gap-2">
                          <AlertTriangle className="h-5 w-5 text-destructive" />
                          Error Details
                        </DialogTitle>
                        <DialogDescription>
                          Error information for package {packageData.name} (v
                          {packageData.version})
                        </DialogDescription>
                      </DialogHeader>
                      <div className="space-y-4">
                        {hasFailures && sampleFailures && (
                          <div className="space-y-3">
                            <div className="text-sm font-semibold text-destructive">
                              Sample Failures ({sampleFailures.length})
                            </div>
                            <div className="border rounded-md divide-y">
                              {sampleFailures.map(
                                (failure: any, index: number) => (
                                  <div
                                    key={index}
                                    className="p-4 hover:bg-muted/50 transition-colors"
                                  >
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                                      <div>
                                        <div className="text-xs font-medium text-muted-foreground mb-1">
                                          Resource Type
                                        </div>
                                        <Badge
                                          variant="outline"
                                          className="font-mono"
                                        >
                                          {failure.resourceType || "N/A"}
                                        </Badge>
                                      </div>
                                      <div>
                                        <div className="text-xs font-medium text-muted-foreground mb-1">
                                          Resource ID
                                        </div>
                                        <div className="text-sm font-mono">
                                          {failure.resourceId || "N/A"}
                                        </div>
                                      </div>
                                      {failure.category && (
                                        <div>
                                          <div className="text-xs font-medium text-muted-foreground mb-1">
                                            Category
                                          </div>
                                          <Badge variant="secondary">
                                            {failure.category}
                                          </Badge>
                                        </div>
                                      )}
                                      <div className="md:col-span-2">
                                        <div className="text-xs font-medium text-muted-foreground mb-1">
                                          Error Message
                                        </div>
                                        <div className="text-sm text-destructive font-medium p-2 bg-destructive/10 rounded border border-destructive/20">
                                          {failure.errorMessage ||
                                            "No error message"}
                                        </div>
                                      </div>
                                    </div>
                                  </div>
                                )
                              )}
                            </div>
                          </div>
                        )}

                        {packageData.errorMessage && (
                          <div className="space-y-2">
                            <div className="text-sm font-semibold text-destructive">
                              Error Message
                            </div>
                            <div className="p-4 border border-destructive/50 rounded-md bg-destructive/5">
                              <pre className="text-sm text-destructive whitespace-pre-wrap wrap-break-words font-mono">
                                {packageData.errorMessage}
                              </pre>
                            </div>
                          </div>
                        )}
                      </div>
                    </DialogContent>
                  </Dialog>
                );
              })()}
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Package ID
              </div>
              <div className="text-base font-mono">{packageData.id}</div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Resource Count
              </div>
              <div className="text-base font-bold">
                {formatNumber(packageData.resourceCount)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Created At
              </div>
              <div className="text-base">
                {formatDate(packageData.createdAt)}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Package Metadata Card */}
      {packageData.metadata &&
        Object.keys(packageData.metadata).length > 0 &&
        (() => {
          const metadata = packageData.metadata as Record<string, any>;
          const manifest = metadata.manifest as Record<string, any> | undefined;
          const resourceTypeFilter = metadata.resource_type_filter as
            | Record<string, any>
            | undefined;

          return (
            <Card>
              <CardHeader>
                <CardTitle>Package Details</CardTitle>
                <CardDescription>
                  Metadata and configuration information
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                {/* Manifest Information */}
                {manifest && (
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    {manifest.author && (
                      <div>
                        <div className="text-xs font-medium text-muted-foreground">
                          Author
                        </div>
                        <div className="text-sm">{manifest.author}</div>
                      </div>
                    )}
                    {manifest.canonical && (
                      <div>
                        <div className="text-xs font-medium text-muted-foreground">
                          Canonical URL
                        </div>
                        <div className="text-sm font-mono break-all">
                          {manifest.canonical}
                        </div>
                      </div>
                    )}
                    {manifest.license && (
                      <div>
                        <div className="text-xs font-medium text-muted-foreground">
                          License
                        </div>
                        <div className="text-sm">
                          <Badge variant="outline">{manifest.license}</Badge>
                        </div>
                      </div>
                    )}
                    {manifest.fhirVersions &&
                      Array.isArray(manifest.fhirVersions) && (
                        <div>
                          <div className="text-xs font-medium text-muted-foreground">
                            FHIR Versions
                          </div>
                          <div className="flex gap-1 flex-wrap">
                            {manifest.fhirVersions.map(
                              (version: string, idx: number) => (
                                <Badge key={idx} variant="secondary">
                                  {version}
                                </Badge>
                              )
                            )}
                          </div>
                        </div>
                      )}
                    {manifest.description && (
                      <div className="md:col-span-2">
                        <div className="text-xs font-medium text-muted-foreground">
                          Description
                        </div>
                        <div className="text-sm text-muted-foreground">
                          {manifest.description}
                        </div>
                      </div>
                    )}
                  </div>
                )}

                {/* Resource Statistics */}
                <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                  {metadata.total_resources !== undefined && (
                    <div className="p-3 bg-muted/30 rounded-md">
                      <div className="text-xs font-medium text-muted-foreground mb-1">
                        Total Resources
                      </div>
                      <div className="text-xl font-bold">
                        {formatNumber(metadata.total_resources)}
                      </div>
                    </div>
                  )}
                  {metadata.installed_resources !== undefined && (
                    <div className="p-3 bg-muted/30 rounded-md">
                      <div className="text-xs font-medium text-muted-foreground mb-1">
                        Installed
                      </div>
                      <div className="text-xl font-bold text-green-600">
                        {formatNumber(metadata.installed_resources)}
                      </div>
                    </div>
                  )}
                  {metadata.filtered_resources !== undefined && (
                    <div className="p-3 bg-muted/30 rounded-md">
                      <div className="text-xs font-medium text-muted-foreground mb-1">
                        Filtered
                      </div>
                      <div className="text-xl font-bold text-orange-600">
                        {formatNumber(metadata.filtered_resources)}
                      </div>
                    </div>
                  )}
                  {metadata.conformance_resources !== undefined && (
                    <div className="p-3 bg-muted/30 rounded-md">
                      <div className="text-xs font-medium text-muted-foreground mb-1">
                        Canonical Resources
                      </div>
                      <div className="text-xl font-bold">
                        {formatNumber(metadata.conformance_resources)}
                      </div>
                    </div>
                  )}
                </div>

                {/* Resource Type Filter */}
                {resourceTypeFilter && (
                  <div>
                    <div className="text-xs font-medium text-muted-foreground mb-2">
                      Resource Type Filter ({resourceTypeFilter.mode})
                    </div>
                    {resourceTypeFilter.resource_types &&
                      Array.isArray(resourceTypeFilter.resource_types) && (
                        <div className="flex gap-1 flex-wrap">
                          {resourceTypeFilter.resource_types.map(
                            (type: string, idx: number) => (
                              <Badge key={idx} variant="outline">
                                {type}
                              </Badge>
                            )
                          )}
                        </div>
                      )}
                  </div>
                )}

                {/* Dependencies */}
                {manifest?.dependencies &&
                  Object.keys(manifest.dependencies).length > 0 && (
                    <div>
                      <div className="text-xs font-medium text-muted-foreground mb-2">
                        Dependencies
                      </div>
                      <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
                        {Object.entries(
                          manifest.dependencies as Record<string, string>
                        ).map(([pkg, version]) => (
                          <div
                            key={pkg}
                            className="p-2 bg-muted/30 rounded-md flex justify-between items-center"
                          >
                            <span className="text-sm font-mono">{pkg}</span>
                            <Badge variant="outline">{version}</Badge>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
              </CardContent>
            </Card>
          );
        })()}

      {/* Resources Table Section */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Resources</CardTitle>
              <CardDescription className="mt-1">
                {formatNumber(total)} total resource
                {total !== 1 ? "s" : ""}
                {filteredResources.length !== resources.length &&
                  ` (${filteredResources.length} shown)`}
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Search Filter */}
          <div className="relative">
            <SearchInput
              searchQuery={resourceFilter}
              setSearchQuery={setResourceFilter}
              placeholder="Filter by resource type or ID..."
            />
          </div>

          {/* Table */}
          <div className="border rounded-md">
            <Table>
              <TableHeader className="sticky top-0 z-10 border-b">
                {table.getHeaderGroups().map((headerGroup) => (
                  <TableRow key={headerGroup.id}>
                    {headerGroup.headers.map((header) => (
                      <TableHead key={header.id} className="text-xs py-2 h-8">
                        {header.isPlaceholder
                          ? null
                          : flexRender(
                              header.column.columnDef.header,
                              header.getContext()
                            )}
                      </TableHead>
                    ))}
                  </TableRow>
                ))}
              </TableHeader>
              <TableBody>
                {filteredResources.length > 0 ? (
                  table.getRowModel().rows.map((row) => {
                    const isExpanded = expandedRows.has(row.index);
                    return (
                      <Fragment key={row.id}>
                        <TableRow>
                          {row.getVisibleCells().map((cell) => (
                            <TableCell key={cell.id} className="py-2 align-top">
                              {flexRender(
                                cell.column.columnDef.cell,
                                cell.getContext()
                              )}
                            </TableCell>
                          ))}
                        </TableRow>
                        {isExpanded && (
                          <TableRow>
                            <TableCell
                              colSpan={columns.length}
                              className="p-4 bg-muted/30"
                            >
                              <div className="space-y-2">
                                <div className="text-xs font-medium text-muted-foreground mb-2">
                                  Resource Content
                                </div>
                                <pre
                                  className={cn(
                                    "text-xs font-mono whitespace-pre-wrap wrap-break-words overflow-auto max-h-96"
                                  )}
                                  dangerouslySetInnerHTML={{
                                    __html: highlightJson(
                                      JSON.stringify(
                                        row.original.resource,
                                        null,
                                        2
                                      )
                                    ),
                                  }}
                                />
                              </div>
                            </TableCell>
                          </TableRow>
                        )}
                      </Fragment>
                    );
                  })
                ) : (
                  <TableRow>
                    <TableCell
                      colSpan={columns.length}
                      className="text-center text-sm text-muted-foreground py-8"
                    >
                      No resources found
                      {resourceFilter && " matching your filter"}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>

          {/* Pagination Controls */}
          {total > 0 && (
            <div className="flex items-center justify-between">
              <div className="text-sm text-muted-foreground">
                Showing {currentOffset + 1} to{" "}
                {Math.min(currentOffset + currentLimit, total)} of {total}{" "}
                resources
                {totalPages > 1 && ` (Page ${currentPage} of ${totalPages})`}
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handlePrevious}
                  disabled={currentOffset === 0}
                >
                  <ChevronLeft className="h-4 w-4 mr-1" />
                  Previous
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleNext}
                  disabled={currentOffset + currentLimit >= total}
                >
                  Next
                  <ChevronRight className="h-4 w-4 ml-1" />
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
};
