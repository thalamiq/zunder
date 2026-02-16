import { useQuery } from "@tanstack/react-query";
import { fetchHealth } from "@/api/health";
import { WifiOff, Loader2, AlertCircle } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import { Button } from "@thalamiq/ui/components/button";
import { formatDistanceToNow } from "date-fns";
import { useState, useEffect } from "react";
import { FHIR_SERVER_URL } from "@/lib/config";
import { LoadingArea } from "./Loading";
import { queryKeys } from "@/api/query-keys";

interface ConnectionGuardProps {
  children: React.ReactNode;
}

export function ConnectionGuard({ children }: ConnectionGuardProps) {
  const [lastCheckTime, setLastCheckTime] = useState<Date | null>(null);
  const [hasEverConnected, setHasEverConnected] = useState(false);

  const { data, isError, isPending, isFetching, dataUpdatedAt, refetch } =
    useQuery({
      queryKey: queryKeys.health,
      queryFn: () => {
        setLastCheckTime(new Date());
        return fetchHealth();
      },
      retry: false,
      refetchInterval: 30000, // Check every 30 seconds
      refetchOnWindowFocus: true,
    });

  const isConnected = !isError && data !== undefined;

  // Track if we've ever successfully connected
  useEffect(() => {
    if (isConnected && !hasEverConnected) {
      setHasEverConnected(true);
    }
  }, [isConnected, hasEverConnected]);

  // Show loading state only on initial load (before we've ever connected)
  if (isPending && !hasEverConnected) {
    return <LoadingArea />;
  }

  // Show info screen when disconnected (but not during refetches if we were previously connected)
  if (!isConnected && !isFetching) {
    const checkTime =
      lastCheckTime || (dataUpdatedAt ? new Date(dataUpdatedAt) : null);
    const timeAgo = checkTime
      ? formatDistanceToNow(checkTime, { addSuffix: true })
      : "Never";

    const handleRetry = () => {
      refetch();
    };

    return (
      <div className="flex items-center justify-center h-full w-full p-6">
        <Card className="w-full max-w-2xl">
          <CardHeader>
            <div className="flex items-center gap-3">
              <WifiOff className="h-6 w-6 text-destructive" />
              <div>
                <CardTitle>Server Connection Unavailable</CardTitle>
                <CardDescription>
                  Unable to connect to the FHIR server
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2 pt-4 border-t">
              <div className="flex items-center justify-between text-sm">
                <span className="text-muted-foreground">Server URL:</span>
                <span className="font-mono text-right break-all">
                  {FHIR_SERVER_URL}
                </span>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span className="text-muted-foreground">Endpoint:</span>
                <span className="font-mono">/health</span>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span className="text-muted-foreground">Last Check:</span>
                <span>{timeAgo}</span>
              </div>
            </div>

            <div className="pt-4">
              <Button
                onClick={handleRetry}
                disabled={isFetching}
                className="w-full"
              >
                {isFetching ? (
                  <>
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                    Retrying...
                  </>
                ) : (
                  "Retry Connection"
                )}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Show children when connected
  return <>{children}</>;
}
