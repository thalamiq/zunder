import { createRoute } from "@tanstack/react-router";
import { useState } from "react";
import { getPackages, installPackageOperation } from "@/api/packages";
import { queryKeys } from "@/api/query-keys";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import { PackagesDisplay } from "@/components/PackagesDisplay";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Button } from "@thalamiq/ui/components/button";
import { Input } from "@thalamiq/ui/components/input";
import { Label } from "@thalamiq/ui/components/label";
import { Checkbox } from "@thalamiq/ui/components/checkbox";
import { Download } from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { rootRoute } from "./root";

function PackagesPage() {
  const queryClient = useQueryClient();
  const [packageName, setPackageName] = useState("");
  const [packageVersion, setPackageVersion] = useState("");
  const [includeExamples, setIncludeExamples] = useState(false);
  const [operationResult, setOperationResult] = useState<string | null>(null);

  const listPackagesQuery = useQuery({
    queryKey: queryKeys.packages(),
    queryFn: () => getPackages(),
  });

  const installMutation = useMutation({
    mutationFn: installPackageOperation,
    onSuccess: (data) => {
      const outcomeParam = data.parameter?.find((p) => p.name === "outcome");
      if (outcomeParam?.resource) {
        const outcome = outcomeParam.resource as {
          issue?: Array<{ diagnostics?: string }>;
        };
        const diagnostics =
          outcome.issue?.[0]?.diagnostics || "Package installation initiated";
        setOperationResult(diagnostics);
      }

      queryClient.invalidateQueries({ queryKey: queryKeys.packages() });

      setPackageName("");
      setPackageVersion("");
      setIncludeExamples(false);
    },
    onError: (error: Error) => {
      setOperationResult(`Error: ${error.message}`);
    },
  });

  const handleInstall = (e: React.FormEvent) => {
    e.preventDefault();
    if (!packageName.trim()) {
      setOperationResult("Error: Package name is required");
      return;
    }

    setOperationResult(null);
    installMutation.mutate({
      name: packageName.trim(),
      version: packageVersion.trim() || undefined,
      includeExamples,
    });
  };

  if (listPackagesQuery.isPending) {
    return <LoadingArea />;
  }

  if (listPackagesQuery.isError) {
    return <ErrorArea error={listPackagesQuery.error} />;
  }

  if (!listPackagesQuery.data) {
    return null;
  }

  return (
    <div className="flex-1 space-y-6 overflow-y-auto p-6">
      <PageHeader
        title="Packages"
        description="Install and manage FHIR packages"
      />
      <Card>
        <CardHeader>
          <CardTitle>Install FHIR Package</CardTitle>
          <CardDescription>
            Download and install a FHIR package from the registry using the
            $install-package operation
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleInstall} className="space-y-4">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="packageName">Package Name *</Label>
                <Input
                  id="packageName"
                  placeholder="e.g., hl7.fhir.r4.core"
                  value={packageName}
                  onChange={(e) => setPackageName(e.target.value)}
                  disabled={installMutation.isPending}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="packageVersion">Version (optional)</Label>
                <Input
                  id="packageVersion"
                  placeholder="e.g., 4.0.1 (leave empty for latest)"
                  value={packageVersion}
                  onChange={(e) => setPackageVersion(e.target.value)}
                  disabled={installMutation.isPending}
                />
              </div>
            </div>

            <div className="flex items-center space-x-2">
              <Checkbox
                id="includeExamples"
                checked={includeExamples}
                onCheckedChange={(checked) =>
                  setIncludeExamples(checked === true)
                }
                disabled={installMutation.isPending}
              />
              <Label htmlFor="includeExamples" className="cursor-pointer">
                Include example resources
              </Label>
            </div>

            <div className="flex items-center gap-4">
              <Button
                type="submit"
                disabled={installMutation.isPending || !packageName.trim()}
              >
                <Download className="h-4 w-4 mr-2" />
                {installMutation.isPending
                  ? "Installing..."
                  : "Install Package"}
              </Button>

              {operationResult && (
                <div
                  className={`text-sm ${
                    operationResult.startsWith("Error")
                      ? "text-destructive"
                      : "text-success"
                  }`}
                >
                  {operationResult}
                </div>
              )}
            </div>
          </form>
        </CardContent>
      </Card>

      <PackagesDisplay data={listPackagesQuery.data} />
    </div>
  );
}

export const packagesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/packages",
  component: PackagesPage,
});
