import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableCell,
  TableHead,
} from "@thalamiq/ui/components/table";
import { Badge } from "@thalamiq/ui/components/badge";
import { Database } from "lucide-react";
import { formatNumber } from "@/lib/utils";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import {
  getSearchIndexTableStatus,
  SearchHashCollisionStatusRecord,
  SearchIndexTableStatusRecord,
  getSearchHashCollisions,
} from "@/api/search";
import { queryKeys } from "@/api/query-keys";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { PageHeader } from "../PageHeader";

const SearchIndexTablesDisplay = () => {
  const indexTablesQuery = useQuery({
    queryKey: queryKeys.searchIndexTableStatus,
    queryFn: () => getSearchIndexTableStatus(),
  });

  const hashCollisionsQuery = useQuery({
    queryKey: queryKeys.searchHashCollisions,
    queryFn: () => getSearchHashCollisions(),
  });

  const indexTableTotals = useMemo(() => {
    const rows = indexTablesQuery.data ?? [];
    const unlogged = rows.filter((r) => r.isUnlogged).length;
    return { tables: rows.length, unlogged };
  }, [indexTablesQuery.data]);

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Index Tables"
        description="Storage footprint and basic health signals"
      />
      <Card>
        <CardContent className="space-y-4 pt-6">
          {indexTablesQuery.isPending ? (
            <LoadingArea />
          ) : indexTablesQuery.isError ? (
            <ErrorArea error={indexTablesQuery.error} />
          ) : (
            <>
              <div className="flex items-center gap-3 text-sm">
                <Database className="w-4 h-4 text-muted-foreground" />
                <div className="text-muted-foreground">
                  {formatNumber(indexTableTotals.tables)} table(s) •{" "}
                  {formatNumber(indexTableTotals.unlogged)} unlogged
                </div>
              </div>
              <div className="rounded-md border overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Table</TableHead>
                      <TableHead className="text-right">Rows</TableHead>
                      <TableHead>UNLOGGED</TableHead>
                      <TableHead className="text-right">Size</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(indexTablesQuery.data ?? []).map(
                      (t: SearchIndexTableStatusRecord) => (
                        <TableRow
                          key={t.tableName}
                          className="hover:bg-accent/30"
                        >
                          <TableCell className="font-mono text-xs">
                            {t.tableName}
                          </TableCell>
                          <TableCell className="text-right font-mono text-xs">
                            {formatNumber(t.rowCount)}
                          </TableCell>
                          <TableCell>
                            {t.isUnlogged ? (
                              <Badge variant="secondary">yes</Badge>
                            ) : (
                              <Badge variant="outline">no</Badge>
                            )}
                          </TableCell>
                          <TableCell className="text-right font-mono text-xs">
                            {t.sizePretty}
                          </TableCell>
                        </TableRow>
                      )
                    )}
                  </TableBody>
                </Table>
              </div>
            </>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Hash collisions</CardTitle>
          <CardDescription>
            Should normally be empty; any rows here indicate potential index
            corruption.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {hashCollisionsQuery.isPending ? (
            <LoadingArea />
          ) : hashCollisionsQuery.isError ? (
            <ErrorArea error={hashCollisionsQuery.error} />
          ) : (hashCollisionsQuery.data ?? []).length === 0 ? (
            <div className="text-sm text-muted-foreground">
              No collisions detected.
            </div>
          ) : (
            <div className="rounded-md border overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Table</TableHead>
                    <TableHead className="text-right">Collisions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {(hashCollisionsQuery.data ?? []).map(
                    (c: SearchHashCollisionStatusRecord) => (
                      <TableRow key={c.tableName}>
                        <TableCell className="font-mono text-xs">
                          {c.tableName}
                        </TableCell>
                        <TableCell className="text-right font-mono text-xs">
                          {formatNumber(c.collisionCount)}
                        </TableCell>
                      </TableRow>
                    )
                  )}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
};

export default SearchIndexTablesDisplay;
