import { Bundle, Resource } from "fhir/r4";
import { useMemo, useState } from "react";
import {
  useReactTable,
  getCoreRowModel,
  ColumnDef,
  flexRender,
} from "@tanstack/react-table";
import {
  Table,
  TableHead,
  TableRow,
  TableHeader,
  TableBody,
  TableCell,
} from "@thalamiq/ui/components/table";
import { cn } from "@thalamiq/ui/utils";
import { highlightJson, highlightPrimitiveValue } from "@/lib/json";

const BundleTableView = ({ bundle }: { bundle: Bundle | Resource }) => {
  // Check if it's a bundle or single resource
  const isBundle = bundle.resourceType === "Bundle";

  // Group resources by resource type
  const groupedResources = useMemo(() => {
    const groups: Record<string, Resource[]> = {};

    if (isBundle) {
      // Handle Bundle
      (bundle as Bundle).entry?.forEach((entry) => {
        if (entry.resource) {
          const resourceType = entry.resource.resourceType;
          if (!groups[resourceType]) {
            groups[resourceType] = [];
          }
          groups[resourceType].push(entry.resource);
        }
      });
    } else {
      // Handle single resource
      const resource = bundle as Resource;
      groups[resource.resourceType] = [resource];
    }

    return groups;
  }, [bundle, isBundle]);

  return (
    <div className="space-y-8">
      {Object.entries(groupedResources).map(([resourceType, resources]) => (
        <ResourceTypeTable
          key={resourceType}
          resourceType={resourceType}
          resources={resources}
        />
      ))}
      {Object.keys(groupedResources).length === 0 && (
        <div className="text-center text-muted-foreground py-8">
          No resources found
        </div>
      )}
    </div>
  );
};

type ResourceTypeTableProps = {
  resourceType: string;
  resources: Resource[];
};

const ResourceTypeTable = ({ resources }: ResourceTypeTableProps) => {
  const [expandedRows, setExpandedRows] = useState<Set<number>>(new Set());

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

  // Get all unique root-level keys from all resources of this type
  const columns = useMemo<ColumnDef<Resource>[]>(() => {
    const allKeys = new Set<string>();
    resources.forEach((resource) => {
      Object.keys(resource).forEach((key) => allKeys.add(key));
    });

    return [
      ...Array.from(allKeys).map((key) => ({
        accessorKey: key,
        header: key,
        cell: ({ row }: { row: { original: Resource; index: number } }) => {
          const value = row.original[key as keyof Resource];
          const isExpanded = expandedRows.has(row.index);

          if (value === null || value === undefined) {
            return <span className="text-muted-foreground text-xs">-</span>;
          }

          if (typeof value === "object") {
            const jsonString = JSON.stringify(value, null, 2);
            const highlighted = highlightJson(jsonString);

            return (
              <pre
                className={cn(
                  "text-xs font-mono max-w-96 whitespace-pre-wrap wrap-break-word leading-tight",
                  !isExpanded && "max-h-80 overflow-hidden"
                )}
                dangerouslySetInnerHTML={{ __html: highlighted }}
              />
            );
          }

          const highlighted = highlightPrimitiveValue(value);
          return (
            <div
              className={cn(
                "text-xs max-w-96 whitespace-normal wrap-break-word leading-tight",
                !isExpanded && "max-h-80 overflow-hidden"
              )}
              dangerouslySetInnerHTML={{ __html: highlighted }}
            />
          );
        },
      })),
    ];
  }, [resources, expandedRows]);

  const table = useReactTable({
    data: resources,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  const rows = table.getRowModel().rows;

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
          {rows.length > 0 ? (
            rows.map((row) => (
              <TableRow key={row.id} onClick={() => toggleRow(row.index)} className="cursor-pointer hover:bg-muted">
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
                colSpan={table.getAllColumns().length}
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

export default BundleTableView;
