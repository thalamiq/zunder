import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { Button } from "@thalamiq/ui/components/button";
import { ChartContainer, ChartConfig } from "@thalamiq/ui/components/chart";
import CustomChartTooltip from "./CustomChartTooltip";
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip } from "recharts";
import {
  FolderIcon,
  Package2Icon,
  ClipboardListIcon,
  CheckCircle2,
  XCircle,
  Clock,
  AlertTriangle,
  ArrowRight,
  Activity,
  Code,
  Loader2,
  PlayIcon,
} from "lucide-react";
import { fetchResources } from "@/api/resources";
import { getQueueHealth, listJobs } from "@/api/jobs";
import { getPackages } from "@/api/packages";
import { queryKeys } from "@/api/query-keys";
import { config } from "@/lib/config";
import { formatDateTime } from "@/lib/utils";
import { fetchMetadata } from "@/api/metadata";
import FhirSearchInput from "@/components/FhirSearchInput";
import ErrorComp from "./ErrorComp";
import { PageHeader } from "./PageHeader";

const formatNumber = (num: number): string => {
  return new Intl.NumberFormat().format(num);
};

const chartConfig = {
  currentTotal: {
    label: "Resources",
    color: "var(--chart-1)",
  },
} satisfies ChartConfig;

const DashboardDisplay = () => {
  const navigate = useNavigate();
  const [endpoint, setEndpoint] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);

  // Fetch capability statement for autocomplete
  const metadataQuery = useQuery({
    queryKey: queryKeys.metadata("full"),
    queryFn: () => fetchMetadata({ mode: "full" }),
  });

  // Handle API request navigation
  const handleApiRequest = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!endpoint.trim()) return;

    setIsSubmitting(true);
    const encodedEndpoint = encodeURIComponent(endpoint.trim());
    navigate({ to: `${config.nav.api.path}?endpoint=${encodedEndpoint}` });
    setTimeout(() => setIsSubmitting(false), 500);
  };

  // Fetch all dashboard data
  const resourcesQuery = useQuery({
    queryKey: queryKeys.resources,
    queryFn: fetchResources,
  });

  const queueHealthQuery = useQuery({
    queryKey: queryKeys.queueHealth,
    queryFn: getQueueHealth,
    refetchInterval: 30000,
  });

  const recentJobsQuery = useQuery({
    queryKey: queryKeys.jobs(undefined, undefined, 5, 0),
    queryFn: () => listJobs({ limit: 5, offset: 0 }),
    refetchInterval: 10000,
  });

  const packagesQuery = useQuery({
    queryKey: queryKeys.packages(),
    queryFn: () => getPackages({ limit: 5 }),
  });

  // Prepare chart data for top resource types
  const topResourceTypes = useMemo(() => {
    if (!resourcesQuery.data?.resourceTypes) return [];
    return [...resourcesQuery.data.resourceTypes]
      .sort((a, b) => b.currentTotal - a.currentTotal)
      .slice(0, 10)
      .map((r) => ({
        resourceType: r.resourceType,
        currentTotal: r.currentTotal,
      }));
  }, [resourcesQuery.data]);

  // Calculate job success rate
  const jobSuccessRate = useMemo(() => {
    if (!queueHealthQuery.data?.stats_24h) return null;
    const { completed, failed, total } = queueHealthQuery.data.stats_24h;
    if (total === 0) return null;
    return Math.round((completed / total) * 100);
  }, [queueHealthQuery.data]);

  // Get queue status badge
  const getQueueStatusBadge = () => {
    const status = queueHealthQuery.data?.status || "unknown";
    const statusLower = status.toLowerCase();

    if (statusLower === "healthy") {
      return (
        <Badge className="bg-green-500/10 text-green-600 border-green-500/20">
          <CheckCircle2 className="w-3 h-3 mr-1" />
          Healthy
        </Badge>
      );
    } else if (statusLower === "degraded") {
      return (
        <Badge className="bg-yellow-500/10 text-yellow-600 border-yellow-500/20">
          <AlertTriangle className="w-3 h-3 mr-1" />
          Degraded
        </Badge>
      );
    } else {
      return (
        <Badge className="bg-red-500/10 text-red-600 border-red-500/20">
          <XCircle className="w-3 h-3 mr-1" />
          {status}
        </Badge>
      );
    }
  };

  const hasError =
    resourcesQuery.isError ||
    queueHealthQuery.isError ||
    recentJobsQuery.isError ||
    packagesQuery.isError ||
    metadataQuery.isError;

  if (hasError) {
    return (
      <ErrorComp
        error={
          resourcesQuery.error ||
          queueHealthQuery.error ||
          recentJobsQuery.error ||
          packagesQuery.error ||
          metadataQuery.error
        }
      />
    );
  }

  const resourcesData = resourcesQuery.data;
  const queueHealthData = queueHealthQuery.data;
  const recentJobs = recentJobsQuery.data?.jobs || [];
  const packages = packagesQuery.data?.packages || [];

  return (
    <div className="flex-1 space-y-6 overflow-y-auto p-6">
      <PageHeader title="Dashboard" />
      {/* API Request Input */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="flex items-center gap-2">
                <Code className="h-5 w-5" />
                API Request
              </CardTitle>
              <CardDescription>
                Make a GET request to any FHIR endpoint
              </CardDescription>
            </div>
            <button
              onClick={() => navigate({ to: config.nav.api.path })}
              className="text-sm text-muted-foreground hover:text-foreground flex items-center gap-1"
            >
              View all <ArrowRight className="h-3 w-3" />
            </button>
          </div>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleApiRequest} className="flex gap-3">
            <div className="flex items-center gap-2 flex-1">
              <Badge
                variant="outline"
                className="px-4 font-mono shrink-0 h-10 rounded-md"
              >
                GET
              </Badge>
              <FhirSearchInput
                searchQuery={endpoint}
                setSearchQuery={setEndpoint}
                inputClassName="h-10 font-mono text-sm"
                capabilityStatement={metadataQuery.data}
              />
            </div>
            <Button
              type="submit"
              disabled={!endpoint.trim() || isSubmitting}
              size="icon"
              className="h-10 w-10 shrink-0"
            >
              {isSubmitting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <PlayIcon className="h-4 w-4" />
              )}
            </Button>
          </form>
        </CardContent>
      </Card>

      {/* Overview Stats */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {/* Resource Types */}
        <Card
          className="cursor-pointer hover:bg-accent/50 transition-colors"
          onClick={() => navigate({ to: config.nav.resources.path })}
        >
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">
              Resource Types
            </CardTitle>
            <FolderIcon className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {resourcesData?.totals.resourceTypeCount || 0}
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              {formatNumber(resourcesData?.totals.currentTotal || 0)} total
              resources
            </p>
          </CardContent>
        </Card>

        {/* Total Resources */}
        <Card
          className="cursor-pointer hover:bg-accent/50 transition-colors"
          onClick={() => navigate({ to: config.nav.resources.path })}
        >
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">
              Active Resources
            </CardTitle>
            <Activity className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-green-600">
              {formatNumber(resourcesData?.totals.currentActive || 0)}
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              {formatNumber(resourcesData?.totals.currentDeleted || 0)} deleted
            </p>
          </CardContent>
        </Card>

        {/* Jobs 24h */}
        <Card
          className="cursor-pointer hover:bg-accent/50 transition-colors"
          onClick={() => navigate({ to: config.nav.jobs.path })}
        >
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">Jobs (24h)</CardTitle>
            <ClipboardListIcon className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {formatNumber(queueHealthData?.stats_24h.total || 0)}
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              {jobSuccessRate !== null
                ? `${jobSuccessRate}% success rate`
                : "No jobs"}
            </p>
          </CardContent>
        </Card>

        {/* Packages */}
        <Card
          className="cursor-pointer hover:bg-accent/50 transition-colors"
          onClick={() => navigate({ to: config.nav.packages.path })}
        >
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">Packages</CardTitle>
            <Package2Icon className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{packages.length}</div>
            <p className="text-xs text-muted-foreground mt-1">
              {packages.filter((p) => p.status === "installed").length}{" "}
              installed
            </p>
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Top Resource Types Chart */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Top Resource Types</CardTitle>
                <CardDescription>Most common resource types</CardDescription>
              </div>
              <button
                onClick={() => navigate({ to: config.nav.resources.path })}
                className="text-sm text-muted-foreground hover:text-foreground flex items-center gap-1"
              >
                View all <ArrowRight className="h-3 w-3" />
              </button>
            </div>
          </CardHeader>
          <CardContent>
            <ChartContainer
              config={chartConfig}
              className="aspect-auto h-[300px] w-full"
            >
              <BarChart data={topResourceTypes}>
                <CartesianGrid vertical={false} strokeDasharray="3 3" />
                <XAxis
                  dataKey="resourceType"
                  tickLine={false}
                  axisLine={false}
                  tickMargin={4}
                  interval={0}
                  className="text-xs"
                  tickFormatter={(value: string) =>
                    value.length > 10 ? value.slice(0, 9) + "\u2026" : value
                  }
                />
                <YAxis
                  tickLine={false}
                  axisLine={false}
                  tickMargin={8}
                  tickFormatter={(value) => formatNumber(value)}
                />
                <Tooltip
                  cursor={{ fill: "hsl(var(--muted))", opacity: 0.1 }}
                  content={(props) => (
                    <CustomChartTooltip {...props} chartConfig={chartConfig} />
                  )}
                />
                <Bar
                  dataKey="currentTotal"
                  fill="var(--color-currentTotal)"
                  radius={[4, 4, 0, 0]}
                />
              </BarChart>
            </ChartContainer>
          </CardContent>
        </Card>

        {/* Job Queue Health */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Job Queue Health</CardTitle>
                <CardDescription>Last 24 hours statistics</CardDescription>
              </div>
              <button
                onClick={() => navigate({ to: config.nav.jobs.path })}
                className="text-sm text-muted-foreground hover:text-foreground flex items-center gap-1"
              >
                View all <ArrowRight className="h-3 w-3" />
              </button>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">Status</span>
              {getQueueStatusBadge()}
            </div>

            {queueHealthData?.stats_24h && (
              <div className="grid grid-cols-2 gap-4 pt-2">
                <div>
                  <div className="text-xs text-muted-foreground">Total</div>
                  <div className="text-lg font-semibold">
                    {formatNumber(queueHealthData.stats_24h.total)}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Pending</div>
                  <div className="text-lg font-semibold flex items-center gap-1">
                    <Clock className="h-3 w-3" />
                    {formatNumber(queueHealthData.stats_24h.pending)}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Running</div>
                  <div className="text-lg font-semibold">
                    {formatNumber(queueHealthData.stats_24h.running)}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Completed</div>
                  <div className="text-lg font-semibold text-green-600 flex items-center gap-1">
                    <CheckCircle2 className="h-3 w-3" />
                    {formatNumber(queueHealthData.stats_24h.completed)}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Failed</div>
                  <div className="text-lg font-semibold text-red-600 flex items-center gap-1">
                    <XCircle className="h-3 w-3" />
                    {formatNumber(queueHealthData.stats_24h.failed)}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Cancelled</div>
                  <div className="text-lg font-semibold">
                    {formatNumber(queueHealthData.stats_24h.cancelled)}
                  </div>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Recent Activity */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Recent Jobs */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Recent Jobs</CardTitle>
                <CardDescription>Latest job activity</CardDescription>
              </div>
              <button
                onClick={() => navigate({ to: config.nav.jobs.path })}
                className="text-sm text-muted-foreground hover:text-foreground flex items-center gap-1"
              >
                View all <ArrowRight className="h-3 w-3" />
              </button>
            </div>
          </CardHeader>
          <CardContent>
            {recentJobs.length === 0 ? (
              <div className="text-sm text-muted-foreground text-center py-4">
                No recent jobs
              </div>
            ) : (
              <div className="space-y-3">
                {recentJobs.map((job) => {
                  const statusLower = job.status.toLowerCase();
                  const statusColor =
                    statusLower === "completed"
                      ? "text-green-600"
                      : statusLower === "failed"
                      ? "text-red-600"
                      : statusLower === "running"
                      ? "text-blue-600"
                      : "text-muted-foreground";

                  return (
                    <div
                      key={job.id}
                      className="flex items-center justify-between p-2 rounded-md hover:bg-accent/50 cursor-pointer"
                      onClick={() => navigate({ to: config.nav.jobs.path })}
                    >
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span
                            className={`text-sm font-medium ${statusColor}`}
                          >
                            {job.jobType}
                          </span>
                          <Badge variant="outline" className="text-xs">
                            {job.status}
                          </Badge>
                        </div>
                        <div className="text-xs text-muted-foreground mt-1 truncate">
                          {job.id}
                        </div>
                      </div>
                      <div className="text-xs text-muted-foreground ml-2">
                        {formatDateTime(job.createdAt)}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Recent Packages */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Recent Packages</CardTitle>
                <CardDescription>Latest package installations</CardDescription>
              </div>
              <button
                onClick={() => navigate({ to: config.nav.packages.path })}
                className="text-sm text-muted-foreground hover:text-foreground flex items-center gap-1"
              >
                View all <ArrowRight className="h-3 w-3" />
              </button>
            </div>
          </CardHeader>
          <CardContent>
            {packages.length === 0 ? (
              <div className="text-sm text-muted-foreground text-center py-4">
                No packages installed
              </div>
            ) : (
              <div className="space-y-3">
                {packages.slice(0, 5).map((pkg) => (
                  <div
                    key={pkg.id}
                    className="flex items-center justify-between p-2 rounded-md hover:bg-accent/50 cursor-pointer"
                    onClick={() => navigate({ to: `/packages/${pkg.id}` })}
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">{pkg.name}</span>
                        <Badge
                          variant="outline"
                          className={
                            pkg.status === "installed"
                              ? "bg-green-500/10 text-green-600 border-green-500/20"
                              : pkg.status === "failed"
                              ? "bg-red-500/10 text-red-600 border-red-500/20"
                              : ""
                          }
                        >
                          {pkg.status}
                        </Badge>
                      </div>
                      <div className="text-xs text-muted-foreground mt-1">
                        v{pkg.version} • {formatNumber(pkg.resourceCount)}{" "}
                        resources
                      </div>
                    </div>
                    <div className="text-xs text-muted-foreground ml-2">
                      {formatDateTime(pkg.createdAt)}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
};

export default DashboardDisplay;
