import { useState, useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
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
import { Switch } from "@thalamiq/ui/components/switch";
import { Label } from "@thalamiq/ui/components/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@thalamiq/ui/components/tabs";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
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
import { RefreshCw, RotateCcw, History, Save, Check } from "lucide-react";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import {
  fetchRuntimeConfig,
  updateRuntimeConfig,
  resetRuntimeConfig,
  fetchRuntimeConfigAudit,
  RuntimeConfigEntry,
  CONFIG_CATEGORIES,
  ConfigCategory,
} from "@/api/config";
import { queryKeys } from "@/api/query-keys";
import { formatDateTime, formatDateTimeFull } from "@/lib/utils";
import { PageHeader } from "@/components/PageHeader";

interface SettingRowProps {
  entry: RuntimeConfigEntry;
  onUpdate: (key: string, value: unknown) => void;
  onReset: (key: string) => void;
  isPending: boolean;
}

const SettingRow = ({ entry, onUpdate, onReset, isPending }: SettingRowProps) => {
  const [localValue, setLocalValue] = useState<unknown>(entry.value);
  const [isDirty, setIsDirty] = useState(false);

  const handleChange = (newValue: unknown) => {
    setLocalValue(newValue);
    setIsDirty(JSON.stringify(newValue) !== JSON.stringify(entry.value));
  };

  const handleSave = () => {
    onUpdate(entry.key, localValue);
    setIsDirty(false);
  };

  const handleReset = () => {
    onReset(entry.key);
    setLocalValue(entry.default_value);
    setIsDirty(false);
  };

  const renderInput = () => {
    switch (entry.value_type) {
      case "boolean":
        return (
          <div className="flex items-center gap-2">
            <Switch
              checked={localValue as boolean}
              onCheckedChange={(checked) => handleChange(checked)}
              disabled={isPending}
            />
            <span className="text-sm text-muted-foreground">
              {localValue ? "Enabled" : "Disabled"}
            </span>
          </div>
        );

      case "integer":
        return (
          <Input
            type="number"
            value={localValue as number}
            onChange={(e) => handleChange(parseInt(e.target.value, 10) || 0)}
            min={entry.min_value}
            max={entry.max_value}
            className="w-32"
            disabled={isPending}
          />
        );

      case "string":
        return (
          <Input
            value={localValue as string}
            onChange={(e) => handleChange(e.target.value)}
            className="w-48"
            disabled={isPending}
          />
        );

      case "string_enum":
        return (
          <Select
            value={localValue as string}
            onValueChange={(value) => handleChange(value)}
            disabled={isPending}
          >
            <SelectTrigger className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {entry.enum_values?.map((value) => (
                <SelectItem key={value} value={value}>
                  {value}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        );

      default:
        return <span className="text-muted-foreground">Unknown type</span>;
    }
  };

  // For boolean values, update immediately on change
  const handleBooleanChange = (checked: boolean) => {
    setLocalValue(checked);
    onUpdate(entry.key, checked);
  };

  return (
    <div className="flex items-center justify-between py-4 border-b last:border-b-0">
      <div className="space-y-1 flex-1">
        <div className="flex items-center gap-2">
          <Label className="font-medium text-sm">{entry.key}</Label>
          {entry.is_default ? (
            <Badge variant="outline" className="text-xs">
              Default
            </Badge>
          ) : (
            <Badge variant="secondary" className="text-xs">
              Custom
            </Badge>
          )}
        </div>
        <p className="text-xs text-muted-foreground">{entry.description}</p>
        {entry.updated_at && (
          <p className="text-xs text-muted-foreground">
            Last updated: {formatDateTime(entry.updated_at)}
            {entry.updated_by && ` by ${entry.updated_by}`}
          </p>
        )}
      </div>
      <div className="flex items-center gap-3">
        {entry.value_type === "boolean" ? (
          <div className="flex items-center gap-2">
            <Switch
              checked={localValue as boolean}
              onCheckedChange={handleBooleanChange}
              disabled={isPending}
            />
            <span className="text-sm text-muted-foreground w-16">
              {localValue ? "Enabled" : "Disabled"}
            </span>
          </div>
        ) : (
          renderInput()
        )}
        {entry.value_type !== "boolean" && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleSave}
            disabled={!isDirty || isPending}
          >
            {isDirty ? <Save className="h-4 w-4" /> : <Check className="h-4 w-4 text-muted-foreground" />}
          </Button>
        )}
        <Button
          variant="ghost"
          size="sm"
          onClick={handleReset}
          disabled={entry.is_default || isPending}
          title="Reset to default"
        >
          <RotateCcw className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
};

const SettingsDisplay = () => {
  const queryClient = useQueryClient();
  const [activeCategory, setActiveCategory] = useState<ConfigCategory>("logging");
  const [auditDialogOpen, setAuditDialogOpen] = useState(false);
  const [auditKey, setAuditKey] = useState<string | undefined>(undefined);

  const configQuery = useQuery({
    queryKey: queryKeys.runtimeConfig(activeCategory),
    queryFn: () => fetchRuntimeConfig(activeCategory),
  });

  const auditQuery = useQuery({
    queryKey: queryKeys.runtimeConfigAudit(auditKey, 50, 0),
    queryFn: () => fetchRuntimeConfigAudit({ key: auditKey, limit: 50 }),
    enabled: auditDialogOpen,
  });

  const updateMutation = useMutation({
    mutationFn: ({ key, value }: { key: string; value: unknown }) =>
      updateRuntimeConfig(key, { value }),
    onSuccess: (_data, variables) => {
      toast.success(`Updated ${variables.key}`);
      queryClient.invalidateQueries({ queryKey: ["runtimeConfig"] });
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to update setting");
    },
  });

  const resetMutation = useMutation({
    mutationFn: (key: string) => resetRuntimeConfig(key),
    onSuccess: (_data, key) => {
      toast.success(`Reset ${key} to default`);
      queryClient.invalidateQueries({ queryKey: ["runtimeConfig"] });
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to reset setting");
    },
  });

  const handleUpdate = (key: string, value: unknown) => {
    updateMutation.mutate({ key, value });
  };

  const handleReset = (key: string) => {
    resetMutation.mutate(key);
  };

  const handleShowAudit = (key?: string) => {
    setAuditKey(key);
    setAuditDialogOpen(true);
  };

  // Group entries by subcategory for interactions
  const groupedEntries = useMemo(() => {
    if (!configQuery.data?.entries) return {};

    if (activeCategory === "interactions") {
      const groups: Record<string, RuntimeConfigEntry[]> = {
        instance: [],
        type: [],
        system: [],
        compartment: [],
        operations: [],
      };

      configQuery.data.entries.forEach((entry) => {
        if (entry.key.includes(".instance.")) groups.instance.push(entry);
        else if (entry.key.includes(".type.")) groups.type.push(entry);
        else if (entry.key.includes(".system.")) groups.system.push(entry);
        else if (entry.key.includes(".compartment.")) groups.compartment.push(entry);
        else if (entry.key.includes(".operations.")) groups.operations.push(entry);
      });

      return groups;
    }

    if (activeCategory === "audit") {
      const groups: Record<string, RuntimeConfigEntry[]> = {
        general: [],
        interactions: [],
      };

      configQuery.data.entries.forEach((entry) => {
        if (entry.key.includes(".interactions.")) groups.interactions.push(entry);
        else groups.general.push(entry);
      });

      return groups;
    }

    return { all: configQuery.data.entries };
  }, [configQuery.data?.entries, activeCategory]);

  if (configQuery.isPending) {
    return <LoadingArea />;
  }

  if (configQuery.isError) {
    return <ErrorArea error={configQuery.error} />;
  }

  const isPending = updateMutation.isPending || resetMutation.isPending;

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Settings"
        description="Runtime configuration settings that can be changed without restarting the server. Changes take effect immediately."
      />
      <Card>
        <CardHeader>
          <div className="flex items-center justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => queryClient.invalidateQueries({ queryKey: ["runtimeConfig"] })}
              >
                <RefreshCw className="w-4 h-4 mr-2" />
                Refresh
              </Button>
              <Button variant="outline" onClick={() => handleShowAudit()}>
                <History className="w-4 h-4 mr-2" />
                Audit Log
              </Button>
          </div>
        </CardHeader>
        <CardContent>
          <Tabs value={activeCategory} onValueChange={(v) => setActiveCategory(v as ConfigCategory)}>
            <TabsList className="mb-4">
              {CONFIG_CATEGORIES.map((cat) => (
                <TabsTrigger key={cat.id} value={cat.id}>
                  {cat.label}
                </TabsTrigger>
              ))}
            </TabsList>

            {CONFIG_CATEGORIES.map((cat) => (
              <TabsContent key={cat.id} value={cat.id} className="space-y-4">
                {Object.entries(groupedEntries).map(([groupName, entries]) => (
                  <Card key={groupName}>
                    <CardHeader className="pb-2">
                      <CardTitle className="text-lg capitalize">
                        {groupName === "all" ? cat.label : groupName}
                      </CardTitle>
                      {groupName !== "all" && (
                        <CardDescription>
                          {groupName === "instance" && "Per-resource instance operations"}
                          {groupName === "type" && "Type-level operations"}
                          {groupName === "system" && "System-level operations"}
                          {groupName === "compartment" && "Compartment search operations"}
                          {groupName === "operations" && "$operation endpoints"}
                          {groupName === "general" && "General audit settings"}
                          {groupName === "interactions" && "Per-interaction audit settings"}
                        </CardDescription>
                      )}
                    </CardHeader>
                    <CardContent>
                      {entries.map((entry) => (
                        <SettingRow
                          key={entry.key}
                          entry={entry}
                          onUpdate={handleUpdate}
                          onReset={handleReset}
                          isPending={isPending}
                        />
                      ))}
                    </CardContent>
                  </Card>
                ))}
              </TabsContent>
            ))}
          </Tabs>
        </CardContent>
      </Card>

      <Dialog open={auditDialogOpen} onOpenChange={setAuditDialogOpen}>
        <DialogContent className="sm:max-w-4xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Configuration Audit Log</DialogTitle>
            <DialogDescription>
              {auditKey
                ? `History of changes for ${auditKey}`
                : "History of all configuration changes"}
            </DialogDescription>
          </DialogHeader>

          {auditQuery.isPending ? (
            <LoadingArea />
          ) : auditQuery.isError ? (
            <ErrorArea error={auditQuery.error} />
          ) : auditQuery.data?.entries.length === 0 ? (
            <div className="py-8 text-center text-muted-foreground">
              No audit entries found.
            </div>
          ) : (
            <div className="rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Time</TableHead>
                    <TableHead>Key</TableHead>
                    <TableHead>Change Type</TableHead>
                    <TableHead>Old Value</TableHead>
                    <TableHead>New Value</TableHead>
                    <TableHead>Changed By</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {auditQuery.data?.entries.map((entry) => (
                    <TableRow key={entry.id}>
                      <TableCell className="text-xs" title={formatDateTimeFull(entry.changed_at)}>
                        {formatDateTime(entry.changed_at)}
                      </TableCell>
                      <TableCell className="font-mono text-xs">{entry.key}</TableCell>
                      <TableCell>
                        <Badge
                          variant={
                            entry.change_type === "create"
                              ? "default"
                              : entry.change_type === "delete"
                              ? "destructive"
                              : entry.change_type === "reset"
                              ? "secondary"
                              : "outline"
                          }
                        >
                          {entry.change_type}
                        </Badge>
                      </TableCell>
                      <TableCell className="font-mono text-xs max-w-[150px] truncate">
                        {entry.old_value !== null ? JSON.stringify(entry.old_value) : "-"}
                      </TableCell>
                      <TableCell className="font-mono text-xs max-w-[150px] truncate">
                        {JSON.stringify(entry.new_value)}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {entry.changed_by || "-"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default SettingsDisplay;
