import { useMemo, useState } from "react";
import { Bundle, Resource } from "fhir/r4";
import {
  useReactTable,
  getCoreRowModel,
  ColumnDef,
  flexRender,
} from "@tanstack/react-table";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Table,
  TableHead,
  TableRow,
  TableHeader,
  TableBody,
  TableCell,
} from "@thalamiq/ui/components/table";
import { Button } from "@thalamiq/ui/components/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@thalamiq/ui/components/dropdown-menu";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@thalamiq/ui/components/alert-dialog";
import { ExternalLink, Copy, MoreVertical, Trash2 } from "lucide-react";
import { highlightPrimitiveValue } from "@/lib/json";
import { RESOURCE_COLUMNS } from "@/lib/defaultColumns";
import { deleteFhirResource } from "@/api/fhir";

// Helper function to extract value from a resource using a simple path
// Returns string, array of strings, or null
// If path encounters an array without an index, extracts from all items
function getValueByPath(
  resource: Resource,
  path: string
): string | string[] | null {
  const parts = path.split(".");

  function traverse(current: any, remainingParts: string[]): any {
    if (current === null || current === undefined) {
      return null;
    }

    if (remainingParts.length === 0) {
      return current;
    }

    const [part, ...rest] = remainingParts;
    const isNumericIndex = /^\d+$/.test(part);

    // If current is an array and part is not a numeric index, process all items
    if (Array.isArray(current) && !isNumericIndex) {
      const results = current
        .map((item) => traverse(item, remainingParts))
        .filter((r) => r !== null && r !== undefined);

      if (results.length === 0) {
        return null;
      }

      // If all results are arrays, flatten them
      const allArrays = results.every((r) => Array.isArray(r));
      if (allArrays) {
        return results.flat();
      }

      return results;
    }

    // Access the property/index
    const next = current[part];
    return traverse(next, rest);
  }

  const result = traverse(resource, parts);

  if (result === null || result === undefined) {
    return null;
  }

  // Handle arrays - return array of stringified values
  if (Array.isArray(result)) {
    if (result.length === 0) {
      return null;
    }
    return result.map((item) => {
      if (item === null || item === undefined) {
        return "";
      }
      if (typeof item === "object") {
        return JSON.stringify(item);
      }
      return String(item);
    });
  }

  // Convert to string
  if (typeof result === "object") {
    return JSON.stringify(result);
  }

  return String(result);
}

// Action cell component to allow hooks
const ActionCell = ({
  resource,
  onDeleted,
}: {
  resource: Resource;
  onDeleted: () => void;
}) => {
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const resourceUrl = `/fhir/${resource.resourceType}/${resource.id}`;

  const deleteMutation = useMutation({
    mutationFn: () =>
      deleteFhirResource(resource.resourceType!, resource.id!),
    onSuccess: onDeleted,
  });

  return (
    <>
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm" className="h-7 px-2">
            <MoreVertical className="h-3 w-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem
            onClick={() => navigator.clipboard.writeText(resourceUrl)}
          >
            <Copy className="mr-2 h-3 w-3" />
            Copy resource URL
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={() => window.open(resourceUrl, "_blank")}
          >
            <ExternalLink className="mr-2 h-3 w-3" />
            Open resource
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            className="text-destructive focus:text-destructive"
            onClick={() => setShowDeleteDialog(true)}
          >
            <Trash2 className="mr-2 h-3 w-3" />
            Delete resource
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <AlertDialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {resource.resourceType}/{resource.id}?</AlertDialogTitle>
            <AlertDialogDescription>
              This will delete the resource. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleteMutation.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              disabled={deleteMutation.isPending}
              onClick={(e) => {
                e.preventDefault();
                deleteMutation.mutate();
              }}
            >
              {deleteMutation.isPending ? "Deleting..." : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
};

// Get columns for a specific resource type
function getColumnsForType(
  resourceType: string,
  onDeleted: () => void
): ColumnDef<Resource>[] {
  const columnConfigs = RESOURCE_COLUMNS[resourceType] || [
    { header: "ID", path: "id" },
    { header: "Last Updated", path: "meta.lastUpdated" },
  ];

  const dataColumns: ColumnDef<Resource>[] = columnConfigs.map((config) => ({
    accessorKey: config.path,
    header: config.header,
    cell: ({ row }) => {
      const value = getValueByPath(row.original, config.path);
      if (value === null) {
        return <span className="text-muted-foreground text-xs">-</span>;
      }

      // Handle arrays - display as bullet list
      if (Array.isArray(value)) {
        return (
          <ul className="text-xs max-w-96 list-disc list-inside space-y-0.5">
            {value.map((item, index) => {
              const highlighted = highlightPrimitiveValue(item);
              return (
                <li
                  key={index}
                  className="leading-tight"
                  dangerouslySetInnerHTML={{ __html: highlighted }}
                />
              );
            })}
          </ul>
        );
      }

      // Handle single values
      const highlighted = highlightPrimitiveValue(value);
      return (
        <div
          className="text-xs max-w-96 whitespace-normal wrap-break-word leading-tight"
          dangerouslySetInnerHTML={{ __html: highlighted }}
        />
      );
    },
  }));

  // Add action column
  const actionColumn: ColumnDef<Resource> = {
    id: "actions",
    header: "Actions",
    cell: ({ row }) => (
      <ActionCell resource={row.original} onDeleted={onDeleted} />
    ),
  };

  return [...dataColumns, actionColumn];
}

interface ResourceTableViewProps {
  data: Bundle | Resource;
  onDeleted?: () => void;
}

// Component for rendering a table for a specific resource type
const ResourceTypeTable = ({
  resourceType,
  resources,
  onDeleted,
}: {
  resourceType: string;
  resources: Resource[];
  onDeleted: () => void;
}) => {
  const columns = useMemo(
    () => getColumnsForType(resourceType, onDeleted),
    [resourceType, onDeleted]
  );

  const table = useReactTable({
    data: resources,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <div>
      <Table>
        <TableHeader className="sticky top-0 z-10 bg-background border-b">
          {table.getHeaderGroups().map((headerGroup) => (
            <TableRow key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <TableHead
                  key={header.id}
                  className="text-xs py-2 h-8 bg-background"
                >
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
          {table.getRowModel().rows.length > 0 ? (
            table.getRowModel().rows.map((row) => (
              <TableRow key={row.id}>
                {row.getVisibleCells().map((cell) => (
                  <TableCell key={cell.id} className="py-2 align-top">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </TableCell>
                ))}
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell
                colSpan={columns.length}
                className="text-center text-xs"
              >
                No resources found
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  );
};

const ResourceTableView = ({ data, onDeleted }: ResourceTableViewProps) => {
  const queryClient = useQueryClient();
  const handleDeleted = onDeleted ?? (() => queryClient.invalidateQueries());
  // Extract all resources from Bundle or single resource
  const resources = useMemo(() => {
    if (data.resourceType === "Bundle") {
      const bundle = data as Bundle;
      const extracted: Resource[] = [];
      bundle.entry?.forEach((entry) => {
        if (entry.resource) {
          extracted.push(entry.resource as Resource);
        }
      });
      return extracted;
    } else {
      return [data as Resource];
    }
  }, [data]);

  // Group resources by type to determine columns
  const resourceTypes = useMemo(() => {
    return Array.from(new Set(resources.map((r) => r.resourceType)));
  }, [resources]);

  // If we have multiple resource types, show them grouped
  if (resourceTypes.length > 1) {
    return (
      <div className="space-y-8">
        {resourceTypes.map((resourceType) => {
          const typeResources = resources.filter(
            (r) => r.resourceType === resourceType
          );
          return (
            <ResourceTypeTable
              key={resourceType}
              resourceType={resourceType}
              resources={typeResources}
              onDeleted={handleDeleted}
            />
          );
        })}
      </div>
    );
  }

  // Single resource type - show single table
  const resourceType = resourceTypes[0] || "Unknown";
  return (
    <ResourceTypeTable resourceType={resourceType} resources={resources} onDeleted={handleDeleted} />
  );
};

export default ResourceTableView;
