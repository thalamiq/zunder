import { useQuery } from "@tanstack/react-query";
import { fetchHealth } from "@/api/health";
import { Wifi, WifiOff, Loader2 } from "lucide-react";
import { Button } from "@thalamiq/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@thalamiq/ui/components/dialog";
import { useState } from "react";
import { formatDistanceToNow } from "date-fns";
import { FHIR_SERVER_URL } from "@/lib/config";
import { cn } from "@thalamiq/ui/utils";
import { fetchMetadata } from "@/api/metadata";
import { queryKeys } from "@/api/query-keys";

export function ConnectionIndicator() {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [lastCheckTime, setLastCheckTime] = useState<Date | null>(null);

  const { data, error, isError, isLoading, dataUpdatedAt } = useQuery({
    queryKey: ["health", "connection-check"],
    queryFn: () => {
      setLastCheckTime(new Date());
      return fetchHealth();
    },
    retry: false,
    refetchInterval: 30000, // Check every 30 seconds
    refetchOnWindowFocus: true,
    enabled: dialogOpen || true, // Always check, but can optimize
  });

  const metadataQuery = useQuery({
    queryKey: queryKeys.metadata("full"),
    queryFn: () => {
      return fetchMetadata({ mode: "full" });
    },
  });

  const isConnected = !isError && data !== undefined;
  const statusText = isLoading
    ? "Checking..."
    : isConnected
    ? "Connected"
    : "Disconnected";

  const getStatusColor = () => {
    if (isLoading) return "text-muted-foreground";
    return isConnected
      ? "text-success hover:text-success"
      : "text-destructive hover:text-destructive";
  };

  const getIcon = () => {
    if (isLoading) {
      return <Loader2 className="h-4 w-4 animate-spin" />;
    }
    return isConnected ? (
      <Wifi className="h-4 w-4" />
    ) : (
      <WifiOff className="h-4 w-4" />
    );
  };

  const checkTime =
    lastCheckTime || (dataUpdatedAt ? new Date(dataUpdatedAt) : null);
  const timeAgo = checkTime
    ? formatDistanceToNow(checkTime, { addSuffix: true })
    : "Never";

  return (
    <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
      <DialogTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          title={`Server: ${statusText}`}
          className={`h-8 w-8 ${getStatusColor()}`}
        >
          {getIcon()}
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Server Connection</DialogTitle>
          <DialogDescription>
            Current connection status and details
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-4">
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium">Status:</span>
            <div className={cn("flex items-center gap-2")}>
              <span className={cn("text-sm font-semibold", getStatusColor())}>
                {statusText}
              </span>
            </div>
          </div>

          {isConnected && metadataQuery.data && (
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">Server Name:</span>
                <span className="text-sm text-muted-foreground">
                  {metadataQuery.data.name || "Unknown"}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">FHIR Version:</span>
                <span className="text-sm text-muted-foreground">
                  {metadataQuery.data.fhirVersion || "Unknown"}
                </span>
              </div>
              {metadataQuery.data.publisher && (
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">Publisher:</span>
                  <span className="text-sm text-muted-foreground">
                    {metadataQuery.data.publisher}
                  </span>
                </div>
              )}
            </div>
          )}

          {isError && (
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">Error:</span>
                <span className="text-sm text-destructive text-right max-w-[60%] wrap-break-word">
                  {error instanceof Error
                    ? error.message
                    : "Unable to connect to the FHIR server"}
                </span>
              </div>
            </div>
          )}

          <div className="space-y-2 pt-4 border-t">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">Server URL:</span>
              <span className="text-sm text-muted-foreground font-mono text-right break-all max-w-[60%]">
                {FHIR_SERVER_URL}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">Endpoint:</span>
              <span className="text-sm text-muted-foreground font-mono">
                /fhir/metadata
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">Last Check:</span>
              <span className="text-sm text-muted-foreground">{timeAgo}</span>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
