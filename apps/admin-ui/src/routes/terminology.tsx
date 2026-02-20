import { createRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { fetchTerminologySummary } from "@/api/terminology";
import { queryKeys } from "@/api/query-keys";
import { ErrorArea } from "@/components/Error";
import { LoadingArea } from "@/components/Loading";
import { TerminologyDisplay } from "@/components/TerminologyDisplay";
import { rootRoute } from "./root";

function TerminologyPage() {
  const query = useQuery({
    queryKey: queryKeys.terminology,
    queryFn: fetchTerminologySummary,
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
    <div className="flex-1 space-y-6 overflow-y-auto p-6">
      <TerminologyDisplay summary={query.data} />
    </div>
  );
}

export const terminologyRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/terminology",
  component: TerminologyPage,
});
