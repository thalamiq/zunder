import { useMemo, useState, useCallback, memo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import { LoadingArea } from "@/components/Loading";
import { ErrorArea } from "@/components/Error";
import { Input } from "@thalamiq/ui/components/input";
import { Button } from "@thalamiq/ui/components/button";
import { Checkbox } from "@thalamiq/ui/components/checkbox";
import { Search } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
import { Combobox, ComboboxOption } from "@/components/Combobox";
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
import { queryKeys } from "@/api/query-keys";
import {
  AdminSearchParameterListItem,
  getAdminSearchParameters,
  getSearchParameterIndexingStatus,
  toggleSearchParameterActive,
} from "@/api/search";
import JsonViewer from "@/components/JsonViewer";
import { formatDate, formatNumber } from "@/lib/utils";
import { PageHeader } from "../PageHeader";

type SearchParameterRow = AdminSearchParameterListItem;

type SearchParameterTableRowProps = {
  sp: SearchParameterRow;
  onSelect: (sp: SearchParameterRow) => void;
  onToggleActive: (id: string) => void;
};

const SearchParameterTableRow = memo(
  ({ sp, onSelect, onToggleActive }: SearchParameterTableRowProps) => {
    const baseString = useMemo(
      () => (sp.base ?? []).join(", ") || "—",
      [sp.base]
    );
    const formattedDate = useMemo(
      () => formatDate(sp.lastUpdated),
      [sp.lastUpdated]
    );
    const isActive = useMemo(() => sp.serverActive ?? false, [sp.serverActive]);

    const handleCheckboxChange = useCallback(() => {
      if (!sp.id) {
        toast.error("Cannot toggle: SearchParameter ID is missing");
        return;
      }
      onToggleActive(sp.id);
    }, [sp.id, onToggleActive]);

    const handleRowClick = useCallback(() => {
      onSelect(sp);
    }, [sp, onSelect]);

    const handleCheckboxClick = useCallback((e: React.MouseEvent) => {
      e.stopPropagation();
    }, []);

    return (
      <TableRow
        className="hover:bg-accent/30 cursor-pointer"
        onClick={handleRowClick}
      >
        <TableCell>
          <Checkbox
            checked={isActive}
            onCheckedChange={handleCheckboxChange}
            onClick={handleCheckboxClick}
          />
        </TableCell>
        <TableCell>
          <div className="space-y-1">
            <div className="font-medium">{sp.code ?? "—"}</div>
          </div>
        </TableCell>
        <TableCell className="text-sm">{sp.type ?? "—"}</TableCell>
        <TableCell className="text-xs text-muted-foreground">
          {baseString}
        </TableCell>
        <TableCell className="text-xs text-muted-foreground">
          {formattedDate}
        </TableCell>
      </TableRow>
    );
  }
);

SearchParameterTableRow.displayName = "SearchParameterTableRow";

const SearchParameterTable = () => {
  const queryClient = useQueryClient();

  const indexingStatusQuery = useQuery({
    queryKey: queryKeys.searchParameterIndexingStatus(),
    queryFn: () => getSearchParameterIndexingStatus(),
    refetchInterval: 30000,
  });

  const [spCount, setSpCount] = useState(100);
  const [spOffset, setSpOffset] = useState(0);
  const [spSearch, setSpSearch] = useState("");
  const [spSearchInput, setSpSearchInput] = useState("");
  const [spTypeFilter, setSpTypeFilter] = useState<string | undefined>(
    undefined
  );
  const [spBaseFilter, setSpBaseFilter] = useState<string | undefined>(
    undefined
  );
  const [selectedSp, setSelectedSp] = useState<SearchParameterRow | null>(null);

  const searchParametersQuery = useQuery({
    queryKey: queryKeys.adminSearchParameters(
      spSearch,
      undefined,
      spTypeFilter,
      spBaseFilter,
      spCount,
      spOffset
    ),
    queryFn: () =>
      getAdminSearchParameters({
        q: spSearch,
        status: undefined,
        type: spTypeFilter,
        resourceType: spBaseFilter,
        limit: spCount,
        offset: spOffset,
      }),
    refetchInterval: 30000,
  });

  const toggleActiveMutation = useMutation({
    mutationFn: async (id: string) => {
      return toggleSearchParameterActive(id);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: queryKeys.searchParameterIndexingStatus(),
      });
      await queryClient.invalidateQueries({
        queryKey: ["adminSearchParameters"],
      });
    },
    onError: (err) => {
      toast.error(
        err instanceof Error
          ? err.message
          : "Failed to toggle SearchParameter active status"
      );
    },
  });

  const searchParameterRows = useMemo(
    () => searchParametersQuery.data?.items ?? [],
    [searchParametersQuery.data]
  );

  const availableBaseTypes = useMemo(() => {
    const set = new Set<string>();
    const coverageRows = indexingStatusQuery.data ?? [];
    for (const row of coverageRows) {
      set.add(row.resourceType);
    }
    set.add("Resource");
    set.add("DomainResource");
    return Array.from(set).sort();
  }, [indexingStatusQuery.data]);

  const baseTypeOptions = useMemo<ComboboxOption[]>(() => {
    return availableBaseTypes.map((rt) => ({
      value: rt,
      label: rt,
    }));
  }, [availableBaseTypes]);

  const handleSpSearch = useCallback(() => {
    setSpSearch(spSearchInput);
    setSpOffset(0);
  }, [spSearchInput]);

  const handleSpSearchKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleSpSearch();
      }
    },
    [handleSpSearch]
  );

  const handleSpSearchInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setSpSearchInput(e.target.value);
    },
    []
  );

  const handleToggleActive = useCallback(
    (id: string) => {
      toggleActiveMutation.mutate(id);
    },
    [toggleActiveMutation]
  );

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Parameters"
        description="Update SearchParameter.status via FHIR PATCH; this triggers the server hook which updates the search parameter tables"
      />
      <Card>
        <CardContent className="space-y-4 pt-6">
          {searchParametersQuery.isPending ? (
            <LoadingArea />
          ) : searchParametersQuery.isError ? (
            <ErrorArea error={searchParametersQuery.error} />
          ) : (
            <>
              <div className="flex flex-col lg:flex-row gap-3">
                <div className="flex-1 flex gap-2">
                  <Input
                    value={spSearchInput}
                    onChange={handleSpSearchInputChange}
                    onKeyDown={handleSpSearchKeyDown}
                    placeholder="Search by code..."
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={handleSpSearch}
                    size="icon"
                  >
                    <Search className="h-4 w-4" />
                  </Button>
                </div>
                <div className="flex gap-2 flex-wrap items-center">
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">Type</span>
                    <Select
                      value={spTypeFilter ?? "__all__"}
                      onValueChange={(value) => {
                        const next = value === "__all__" ? undefined : value;
                        setSpTypeFilter(next);
                        setSpOffset(0);
                      }}
                    >
                      <SelectTrigger className="h-9 w-[140px]">
                        <SelectValue placeholder="All" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="__all__">All</SelectItem>
                        {[
                          "string",
                          "number",
                          "date",
                          "token",
                          "reference",
                          "composite",
                          "quantity",
                          "uri",
                          "special",
                        ].map((t) => (
                          <SelectItem key={t} value={t}>
                            {t}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">Base</span>
                    <Combobox
                      options={baseTypeOptions}
                      value={spBaseFilter ?? "__all__"}
                      onValueChange={(value) => {
                        setSpBaseFilter(value);
                        setSpOffset(0);
                      }}
                      placeholder="All"
                      allOptionLabel="All"
                      allOptionValue="__all__"
                      width="200px"
                    />
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">
                      Page size
                    </span>
                    <Select
                      value={String(spCount)}
                      onValueChange={(value) => {
                        setSpCount(Number(value));
                        setSpOffset(0);
                      }}
                    >
                      <SelectTrigger className="h-9 w-[100px]">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {[50, 100, 200, 500].map((n) => (
                          <SelectItem key={n} value={String(n)}>
                            {n}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </div>

              {searchParameterRows.length === 0 ? (
                <div className="text-sm text-muted-foreground text-center py-10">
                  No SearchParameters match your filters.
                </div>
              ) : (
                <>
                  <div className="text-sm text-muted-foreground">
                    Showing {formatNumber(searchParameterRows.length)} of{" "}
                    {typeof searchParametersQuery.data?.total === "number"
                      ? formatNumber(searchParametersQuery.data.total)
                      : "—"}{" "}
                    SearchParameters
                  </div>
                  <div className="rounded-md border overflow-x-auto">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Active</TableHead>
                          <TableHead>Code</TableHead>
                          <TableHead>Type</TableHead>
                          <TableHead>Base</TableHead>
                          <TableHead>Updated</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {searchParameterRows.map((sp) => (
                          <SearchParameterTableRow
                            key={sp.id}
                            sp={sp}
                            onSelect={setSelectedSp}
                            onToggleActive={handleToggleActive}
                          />
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                </>
              )}

              <div className="flex items-center justify-between pt-2">
                <div className="text-xs text-muted-foreground">
                  Showing {formatNumber(searchParameterRows.length)} of{" "}
                  {typeof searchParametersQuery.data?.total === "number"
                    ? formatNumber(searchParametersQuery.data.total)
                    : "—"}{" "}
                  SearchParameters
                </div>
                <div className="flex items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={spOffset <= 0}
                    onClick={() => setSpOffset(Math.max(0, spOffset - spCount))}
                  >
                    Previous
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={
                      typeof searchParametersQuery.data?.total === "number"
                        ? spOffset + spCount >= searchParametersQuery.data.total
                        : (searchParametersQuery.data?.items?.length ?? 0) <
                        spCount
                    }
                    onClick={() => setSpOffset(spOffset + spCount)}
                  >
                    Next
                  </Button>
                </div>
              </div>
            </>
          )}
        </CardContent>
      </Card>

      <Dialog
        open={!!selectedSp}
        onOpenChange={(o) => (!o ? setSelectedSp(null) : null)}
      >
        <DialogContent className="sm:max-w-4xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>SearchParameter details</DialogTitle>
            <DialogDescription className="font-mono text-xs break-all">
              {selectedSp?.url ?? selectedSp?.id ?? ""}
            </DialogDescription>
          </DialogHeader>
          {selectedSp && (
            <div className="space-y-4">
              <JsonViewer data={selectedSp} />
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setSelectedSp(null)}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default SearchParameterTable;
