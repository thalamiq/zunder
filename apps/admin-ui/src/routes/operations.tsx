import { createRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { fetchOperations } from "@/api/operations";
import { queryKeys } from "@/api/query-keys";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import { OperationsDisplay } from "@/components/OperationsDisplay";
import { rootRoute } from "./root";

function OperationsPage() {
  const query = useQuery({
    queryKey: queryKeys.operations,
    queryFn: fetchOperations,
  });

  if (query.isPending) {
    return <LoadingArea />;
  }

  if (query.isError) {
    return <ErrorArea error={query.error} />;
  }

  if (!query.data) {
    return null;
  }

  return (
    <div className="space-y-6 p-6">
      <OperationsDisplay operations={query.data} />
    </div>
  );
}

export const operationsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/operations",
  component: OperationsPage,
});
