import { useMemo, useState, useCallback } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { formatDate } from "@/lib/utils";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableCell,
  TableHead,
} from "@thalamiq/ui/components/table";
import { Input } from "@thalamiq/ui/components/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@thalamiq/ui/components/dialog";
import { Button } from "@thalamiq/ui/components/button";
import { Layers, FileCode2, Tags } from "lucide-react";
import { queryKeys } from "@/api/query-keys";
import {
  getCompartmentMemberships,
  CompartmentMembershipRecord,
} from "@/api/search";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import JsonViewer from "@/components/JsonViewer";
import { Combobox } from "@/components/Combobox";
import { PageHeader } from "../PageHeader";

const SearchCompartmentsDisplay = () => {
  const compartmentsQuery = useQuery({
    queryKey: queryKeys.compartmentMemberships,
    queryFn: () => getCompartmentMemberships(),
    refetchInterval: 30000,
  });

  const [compartmentTypeFilter, setCompartmentTypeFilter] = useState<
    string | undefined
  >(undefined);
  const [resourceTypeFilter, setResourceTypeFilter] = useState("");
  const [selectedMembership, setSelectedMembership] =
    useState<CompartmentMembershipRecord | null>(null);

  const memberships = useMemo(
    () => compartmentsQuery.data ?? [],
    [compartmentsQuery.data]
  );

  const summary = useMemo(() => {
    const compartmentTypes = new Set(
      memberships.map((m) => m.compartmentType)
    ).size;
    const resourceTypes = new Set(memberships.map((m) => m.resourceType)).size;
    const totalParameters = memberships.reduce(
      (acc, m) => acc + m.parameterNames.length,
      0
    );
    const mostRecent =
      memberships.length > 0
        ? memberships.reduce((latest, m) =>
            new Date(m.loadedAt) > new Date(latest.loadedAt) ? m : latest
          ).loadedAt
        : null;

    return {
      compartmentTypes,
      resourceTypes,
      totalParameters,
      totalMemberships: memberships.length,
      mostRecent,
    };
  }, [memberships]);

  const availableCompartmentTypes = useMemo(() => {
    const types = new Set(memberships.map((m) => m.compartmentType));
    return Array.from(types).sort();
  }, [memberships]);

  const compartmentTypeOptions = useMemo(() => {
    return availableCompartmentTypes.map((ct) => ({
      value: ct,
      label: ct,
    }));
  }, [availableCompartmentTypes]);

  const filteredMemberships = useMemo(() => {
    let filtered = memberships;

    if (compartmentTypeFilter) {
      filtered = filtered.filter(
        (m) => m.compartmentType === compartmentTypeFilter
      );
    }

    if (resourceTypeFilter.trim()) {
      const needle = resourceTypeFilter.trim().toLowerCase();
      filtered = filtered.filter((m) =>
        m.resourceType.toLowerCase().includes(needle)
      );
    }

    return filtered;
  }, [memberships, compartmentTypeFilter, resourceTypeFilter]);

  const handleRowClick = useCallback((membership: CompartmentMembershipRecord) => {
    setSelectedMembership(membership);
  }, []);

  if (compartmentsQuery.isPending) {
    return <LoadingArea />;
  }

  if (compartmentsQuery.isError) {
    return <ErrorArea error={compartmentsQuery.error} />;
  }

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Compartments"
        description="Compartment search enables access control by resource ownership"
      />
      <Card>
        <CardContent className="pt-6">
          <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
            <div>
              <div className="text-sm font-medium text-muted-foreground flex items-center gap-2">
                <Layers className="w-4 h-4" />
                Compartment types
              </div>
              <div className="text-2xl font-bold">
                {summary.compartmentTypes}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground flex items-center gap-2">
                <FileCode2 className="w-4 h-4" />
                Resource types
              </div>
              <div className="text-2xl font-bold">{summary.resourceTypes}</div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Memberships
              </div>
              <div className="text-2xl font-bold">
                {summary.totalMemberships}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground flex items-center gap-2">
                <Tags className="w-4 h-4" />
                Parameters
              </div>
              <div className="text-2xl font-bold">{summary.totalParameters}</div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Last loaded
              </div>
              <div className="text-sm font-medium">
                {summary.mostRecent ? formatDate(summary.mostRecent) : "—"}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Compartment Memberships</CardTitle>
          <CardDescription>
            Resource types and their membership parameters per compartment
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col lg:flex-row gap-3">
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground whitespace-nowrap">
                Compartment
              </span>
              <Combobox
                options={compartmentTypeOptions}
                value={compartmentTypeFilter ?? "__all__"}
                onValueChange={(value) => setCompartmentTypeFilter(value)}
                placeholder="All"
                allOptionLabel="All"
                allOptionValue="__all__"
                width="180px"
              />
            </div>
            <div className="flex-1">
              <Input
                value={resourceTypeFilter}
                onChange={(e) => setResourceTypeFilter(e.target.value)}
                placeholder="Filter by resource type..."
              />
            </div>
          </div>

          {filteredMemberships.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-10">
              No compartment memberships match your filters.
            </div>
          ) : (
            <div className="rounded-md border overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Compartment</TableHead>
                    <TableHead>Resource Type</TableHead>
                    <TableHead>Parameters</TableHead>
                    <TableHead>Temporal</TableHead>
                    <TableHead>Loaded At</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredMemberships.map((membership, idx) => (
                    <TableRow
                      key={`${membership.compartmentType}-${membership.resourceType}-${idx}`}
                      className="hover:bg-accent/30 cursor-pointer"
                      onClick={() => handleRowClick(membership)}
                    >
                      <TableCell className="font-medium">
                        <Badge variant="outline" className="font-mono">
                          {membership.compartmentType}
                        </Badge>
                      </TableCell>
                      <TableCell className="font-medium">
                        {membership.resourceType}
                      </TableCell>
                      <TableCell>
                        <div className="flex flex-wrap gap-1">
                          {membership.parameterNames.map((param) => (
                            <Badge
                              key={param}
                              variant={
                                param === "{def}" ? "default" : "secondary"
                              }
                              className="text-xs"
                            >
                              {param}
                            </Badge>
                          ))}
                        </div>
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {membership.startParam || membership.endParam ? (
                          <div className="space-y-0.5">
                            {membership.startParam && (
                              <div>start: {membership.startParam}</div>
                            )}
                            {membership.endParam && (
                              <div>end: {membership.endParam}</div>
                            )}
                          </div>
                        ) : (
                          "—"
                        )}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {formatDate(membership.loadedAt)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}

          <div className="text-xs text-muted-foreground space-y-1">
            <p>
              Compartments are loaded from CompartmentDefinition resources.
              Parameter <Badge variant="default" className="text-xs mx-1">{"{def}"}</Badge> means the
              compartment resource itself.
            </p>
            <p>
              Multiple parameters are ORed together: a resource is in the
              compartment if ANY parameter matches the compartment ID.
            </p>
          </div>
        </CardContent>
      </Card>

      <Dialog
        open={!!selectedMembership}
        onOpenChange={(o) => (!o ? setSelectedMembership(null) : null)}
      >
        <DialogContent className="sm:max-w-4xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Membership details</DialogTitle>
            <DialogDescription className="font-mono text-xs">
              {selectedMembership
                ? `${selectedMembership.compartmentType} / ${selectedMembership.resourceType}`
                : ""}
            </DialogDescription>
          </DialogHeader>
          {selectedMembership && (
            <div className="space-y-4">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-sm">
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Compartment Info</CardTitle>
                    <CardDescription>Type and resource</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">
                        Compartment Type
                      </span>
                      <Badge variant="outline" className="font-mono">
                        {selectedMembership.compartmentType}
                      </Badge>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">
                        Resource Type
                      </span>
                      <span className="font-medium">
                        {selectedMembership.resourceType}
                      </span>
                    </div>
                    <div className="text-muted-foreground pt-2">Loaded At</div>
                    <div>{formatDate(selectedMembership.loadedAt)}</div>
                  </CardContent>
                </Card>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">Parameters</CardTitle>
                    <CardDescription>Membership criteria</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="text-muted-foreground">
                      Parameter Names ({selectedMembership.parameterNames.length})
                    </div>
                    <div className="flex flex-wrap gap-1">
                      {selectedMembership.parameterNames.map((param) => (
                        <Badge
                          key={param}
                          variant={param === "{def}" ? "default" : "secondary"}
                        >
                          {param}
                        </Badge>
                      ))}
                    </div>
                    {(selectedMembership.startParam ||
                      selectedMembership.endParam) && (
                      <>
                        <div className="text-muted-foreground pt-2">
                          Temporal Bounds
                        </div>
                        <div className="space-y-1">
                          {selectedMembership.startParam && (
                            <div className="flex justify-between">
                              <span className="text-muted-foreground">
                                Start Param
                              </span>
                              <Badge variant="outline">
                                {selectedMembership.startParam}
                              </Badge>
                            </div>
                          )}
                          {selectedMembership.endParam && (
                            <div className="flex justify-between">
                              <span className="text-muted-foreground">
                                End Param
                              </span>
                              <Badge variant="outline">
                                {selectedMembership.endParam}
                              </Badge>
                            </div>
                          )}
                        </div>
                      </>
                    )}
                  </CardContent>
                </Card>
              </div>
              <div className="rounded-md border">
                <div className="px-4 py-2 border-b text-sm font-medium">
                  Raw Data
                </div>
                <JsonViewer data={selectedMembership} />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setSelectedMembership(null)}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default SearchCompartmentsDisplay;
