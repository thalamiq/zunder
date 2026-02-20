import { useMemo, useState } from "react";
import type { TerminologySummary, CodeSystemSummary } from "@/api/terminology";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { Button } from "@thalamiq/ui/components/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
import SearchInput from "@/components/SearchInput";
import { ArrowUpDown } from "lucide-react";
import { PageHeader } from "./PageHeader";

function StatCard({
  label,
  value,
  detail,
}: {
  label: string;
  value: number | string;
  detail?: string;
}) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <p className="text-xs font-medium text-muted-foreground">{label}</p>
      </CardHeader>
      <CardContent>
        <p className="text-2xl font-semibold tabular-nums">{value}</p>
        {detail && (
          <p className="text-xs text-muted-foreground mt-1">{detail}</p>
        )}
      </CardContent>
    </Card>
  );
}

type SortField = "url" | "conceptCount";
type SortDir = "asc" | "desc";

function CodeSystemsSection({
  codesystems,
}: {
  codesystems: CodeSystemSummary[];
}) {
  const [search, setSearch] = useState("");
  const [sortField, setSortField] = useState<SortField>("conceptCount");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [pageSize, setPageSize] = useState(25);
  const [offset, setOffset] = useState(0);

  const filtered = useMemo(() => {
    let result = codesystems;
    if (search.trim()) {
      const needle = search.trim().toLowerCase();
      result = result.filter((cs) => cs.url.toLowerCase().includes(needle));
    }
    result = [...result].sort((a, b) => {
      if (sortField === "url") {
        const cmp = a.url.localeCompare(b.url);
        return sortDir === "asc" ? cmp : -cmp;
      }
      const cmp = a.conceptCount - b.conceptCount;
      return sortDir === "asc" ? cmp : -cmp;
    });
    return result;
  }, [codesystems, search, sortField, sortDir]);

  const page = filtered.slice(offset, offset + pageSize);
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.floor(offset / pageSize) + 1;

  const toggleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortField(field);
      setSortDir(field === "url" ? "asc" : "desc");
    }
    setOffset(0);
  };

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base">CodeSystems</CardTitle>
          <span className="text-xs text-muted-foreground tabular-nums">
            {filtered.length === codesystems.length
              ? `${codesystems.length} systems`
              : `${filtered.length} of ${codesystems.length} systems`}
          </span>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {codesystems.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No CodeSystem concepts indexed.
          </p>
        ) : (
          <>
            <div className="flex items-center gap-2">
              <SearchInput
                searchQuery={search}
                setSearchQuery={(v) => {
                  setSearch(v);
                  setOffset(0);
                }}
                placeholder="Filter by system URL..."
              />
            </div>

            <div className="border rounded-md">
              <table className="w-full text-left">
                <thead>
                  <tr className="border-b bg-muted/40 text-xs text-muted-foreground">
                    <th className="py-2 px-3 font-medium">
                      <button
                        type="button"
                        className="flex items-center gap-1 hover:text-foreground transition-colors"
                        onClick={() => toggleSort("url")}
                      >
                        System URL
                        <ArrowUpDown className="h-3 w-3" />
                      </button>
                    </th>
                    <th className="py-2 px-3 font-medium text-right">
                      <button
                        type="button"
                        className="flex items-center gap-1 ml-auto hover:text-foreground transition-colors"
                        onClick={() => toggleSort("conceptCount")}
                      >
                        Concepts
                        <ArrowUpDown className="h-3 w-3" />
                      </button>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {page.length === 0 ? (
                    <tr>
                      <td
                        colSpan={2}
                        className="py-6 text-center text-sm text-muted-foreground"
                      >
                        No systems match your filter.
                      </td>
                    </tr>
                  ) : (
                    page.map((cs) => (
                      <tr
                        key={cs.url}
                        className="border-b border-border/50 text-sm hover:bg-muted/30 transition-colors"
                      >
                        <td className="py-2 px-3 font-mono text-xs truncate max-w-lg">
                          {cs.url}
                        </td>
                        <td className="py-2 px-3 text-right tabular-nums">
                          {cs.conceptCount.toLocaleString()}
                        </td>
                      </tr>
                    ))
                  )}
                </tbody>
              </table>
            </div>

            {/* Pagination */}
            {filtered.length > pageSize && (
              <div className="flex items-center justify-between text-xs text-muted-foreground pt-1">
                <div className="flex items-center gap-2">
                  <span>Rows per page</span>
                  <Select
                    value={String(pageSize)}
                    onValueChange={(v) => {
                      setPageSize(Number(v));
                      setOffset(0);
                    }}
                  >
                    <SelectTrigger className="h-7 w-[70px] text-xs">
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
                <div className="flex items-center gap-2">
                  <span className="tabular-nums">
                    Page {currentPage} of {totalPages}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 text-xs"
                    disabled={offset <= 0}
                    onClick={() => setOffset(Math.max(0, offset - pageSize))}
                  >
                    Previous
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 text-xs"
                    disabled={offset + pageSize >= filtered.length}
                    onClick={() => setOffset(offset + pageSize)}
                  >
                    Next
                  </Button>
                </div>
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}

export function TerminologyDisplay({
  summary,
}: {
  summary: TerminologySummary;
}) {
  return (
    <div className="space-y-6">
      <PageHeader
        title="Terminology"
        description="CodeSystems, ValueSets, and expansion cache"
      />
      {/* Summary stat cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard
          label="CodeSystems"
          value={summary.codesystems.length}
        />
        <StatCard
          label="Total Concepts"
          value={summary.totalConcepts.toLocaleString()}
        />
        <StatCard
          label="Cached Expansions"
          value={summary.cachedExpansions}
          detail={`${summary.activeExpansions} active`}
        />
        <StatCard
          label="ConceptMaps"
          value={summary.conceptmapCount}
        />
      </div>

      {/* CodeSystems table with search + sort + pagination */}
      <CodeSystemsSection codesystems={summary.codesystems} />

      {/* ValueSets */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <CardTitle className="text-base">ValueSets</CardTitle>
            <Badge variant="secondary" className="text-xs tabular-nums">
              {summary.valuesetCount.toLocaleString()}
            </Badge>
          </div>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <p className="text-xs text-muted-foreground">Resources stored</p>
              <p className="font-semibold tabular-nums">
                {summary.valuesetCount.toLocaleString()}
              </p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">
                Expansion cache
              </p>
              <p className="font-semibold tabular-nums">
                {summary.cachedExpansions.toLocaleString()}
                <span className="font-normal text-muted-foreground text-xs ml-1">
                  ({summary.activeExpansions} active)
                </span>
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Closure tables */}
      {summary.closureTables.length > 0 && (
        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-base">Closure Tables</CardTitle>
              <span className="text-xs text-muted-foreground tabular-nums">
                {summary.closureTables.length} table
                {summary.closureTables.length !== 1 ? "s" : ""}
              </span>
            </div>
          </CardHeader>
          <CardContent>
            <div className="border rounded-md">
              <table className="w-full text-left">
                <thead>
                  <tr className="border-b bg-muted/40 text-xs text-muted-foreground">
                    <th className="py-2 px-3 font-medium">Name</th>
                    <th className="py-2 px-3 font-medium text-right">
                      Version
                    </th>
                    <th className="py-2 px-3 font-medium text-right">
                      Concepts
                    </th>
                    <th className="py-2 px-3 font-medium text-right">
                      Relations
                    </th>
                    <th className="py-2 px-3 font-medium">Status</th>
                  </tr>
                </thead>
                <tbody>
                  {summary.closureTables.map((ct) => (
                    <tr
                      key={ct.name}
                      className="border-b border-border/50 text-sm hover:bg-muted/30 transition-colors"
                    >
                      <td className="py-2 px-3 font-mono text-xs">
                        {ct.name}
                      </td>
                      <td className="py-2 px-3 text-right tabular-nums">
                        {ct.currentVersion}
                      </td>
                      <td className="py-2 px-3 text-right tabular-nums">
                        {ct.conceptCount.toLocaleString()}
                      </td>
                      <td className="py-2 px-3 text-right tabular-nums">
                        {ct.relationCount.toLocaleString()}
                      </td>
                      <td className="py-2 px-3">
                        {ct.requiresReinit ? (
                          <Badge variant="destructive" className="text-xs">
                            Reinit needed
                          </Badge>
                        ) : (
                          <Badge variant="secondary" className="text-xs">
                            OK
                          </Badge>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
