"use client";

import { useState, useMemo } from "react";
import { ChartContainer, ChartConfig } from "@thalamiq/ui/components/chart";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip } from "recharts";
import SearchInput from "./SearchInput";
import { ResourceTypeStatsReport } from "@/lib/api/resources";
import { useRouter } from "next/navigation";
import { config } from "@/lib/config";
import CustomChartTooltip from "./CustomChartTooltip";
import { formatDate, formatNumber } from "@/lib/utils";

interface ResourcesDisplayProps {
  report: ResourceTypeStatsReport;
}

const chartConfig = {
  currentTotal: {
    label: "Current Total",
    color: "var(--chart-1)",
  },
  currentActive: {
    label: "Active",
    color: "var(--chart-2)",
  },
  currentDeleted: {
    label: "Deleted",
    color: "var(--chart-3)",
  },
} satisfies ChartConfig;

export const ResourcesDisplay = ({ report }: ResourcesDisplayProps) => {
  const router = useRouter();

  const { resourceTypes, totals } = report;
  const [resourceFilter, setResourceFilter] = useState("");

  // Filter function for resources
  const filteredResources = useMemo(() => {
    if (!resourceFilter) return resourceTypes;
    const filter = resourceFilter.toLowerCase();
    return resourceTypes.filter((r) =>
      r.resourceType.toLowerCase().includes(filter)
    );
  }, [resourceTypes, resourceFilter]);

  // Prepare chart data from filtered resources
  const chartData = useMemo(() => {
    return filteredResources
      .map((resource) => ({
        resourceType: resource.resourceType,
        currentTotal: resource.currentTotal,
        currentActive: resource.currentActive,
        currentDeleted: resource.currentDeleted,
      }))
      .sort((a, b) => b.currentTotal - a.currentTotal)
      .slice(0, 20); // Limit to top 20 for better visualization
  }, [filteredResources]);

  return (
    <div className="space-y-4 p-6">
      {/* Header Section */}
      <Card>
        <CardHeader>
          <CardTitle>Resource Statistics</CardTitle>
          <CardDescription>
            Overview of all resource types and their statistics
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-4">
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Resource Types
              </div>
              <div className="text-2xl font-bold">
                {totals.resourceTypeCount}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Total Versions
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(totals.totalVersions)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Current Total
              </div>
              <div className="text-2xl font-bold">
                {formatNumber(totals.currentTotal)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Active
              </div>
              <div className="text-2xl font-bold text-success">
                {formatNumber(totals.currentActive)}
              </div>
            </div>
            <div>
              <div className="text-sm font-medium text-muted-foreground">
                Deleted
              </div>
              <div className="text-2xl font-bold text-destructive">
                {formatNumber(totals.currentDeleted)}
              </div>
            </div>
          </div>
          {totals.lastUpdated && (
            <div className="mt-4 text-sm text-muted-foreground">
              Last Updated: {formatDate(totals.lastUpdated)}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Bar Chart */}
      {filteredResources.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Resource Distribution</CardTitle>
            <CardDescription>
              Current totals for resource types
              {chartData.length < filteredResources.length &&
                ` (showing top ${chartData.length})`}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <ChartContainer
              config={chartConfig}
              className="aspect-auto h-[400px] w-full"
            >
              <BarChart data={chartData}>
                <CartesianGrid vertical={false} />
                <XAxis
                  dataKey="resourceType"
                  tickLine={false}
                  axisLine={false}
                  tickMargin={8}
                  interval={0}
                  className="text-xs"
                  tickFormatter={(value: string) =>
                    value.length > 12 ? value.slice(0, 11) + "\u2026" : value
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
                  content={
                    <CustomChartTooltip
                      payload={[]}
                      label=""
                      chartConfig={chartConfig}
                    />
                  }
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
      )}

      {/* Resource Types Table */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Resource Types</CardTitle>
              <CardDescription className="mt-1">
                {filteredResources.length} resource type
                {filteredResources.length !== 1 ? "s" : ""}
                {filteredResources.length !== resourceTypes.length &&
                  ` of ${resourceTypes.length}`}
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="relative">
            <SearchInput
              searchQuery={resourceFilter}
              setSearchQuery={setResourceFilter}
              placeholder="Filter resource types..."
            />
          </div>

          {filteredResources.length === 0 ? (
            <div className="text-sm text-muted-foreground text-center py-8">
              No resource types match your filter.
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-4">
              {filteredResources.map((resource) => (
                <Card
                  key={resource.resourceType}
                  className="cursor-pointer hover:bg-accent/50 transition-colors"
                  onClick={() =>
                    router.push(
                      `${config.nav.api.path}?endpoint=${resource.resourceType}`
                    )
                  }
                >
                  <CardHeader className="pb-3">
                    <CardTitle className="text-base">
                      {resource.resourceType}
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="space-y-3">
                    <div className="space-y-2 text-sm">
                      <div className="flex justify-between items-center">
                        <span className="text-muted-foreground">
                          Total Versions:
                        </span>
                        <span className="font-medium">
                          {formatNumber(resource.totalVersions)}
                        </span>
                      </div>
                      <div className="flex justify-between items-center">
                        <span className="text-muted-foreground">
                          Current Total:
                        </span>
                        <span className="font-medium">
                          {formatNumber(resource.currentTotal)}
                        </span>
                      </div>
                      <div className="flex justify-between items-center">
                        <span className="text-muted-foreground">Active:</span>
                        <Badge
                          variant="outline"
                          className="bg-success/10 text-success border-success/20"
                        >
                          {formatNumber(resource.currentActive)}
                        </Badge>
                      </div>
                      <div className="flex justify-between items-center">
                        <span className="text-muted-foreground">Deleted:</span>
                        {resource.currentDeleted > 0 ? (
                          <Badge
                            variant="outline"
                            className="bg-destructive/10 text-destructive border-destructive/20"
                          >
                            {formatNumber(resource.currentDeleted)}
                          </Badge>
                        ) : (
                          <span className="text-muted-foreground">
                            {formatNumber(resource.currentDeleted)}
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="pt-2 border-t">
                      <div className="text-xs text-muted-foreground">
                        <div className="font-medium mb-1">Last Updated:</div>
                        <div>{formatDate(resource.lastUpdated)}</div>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
};
