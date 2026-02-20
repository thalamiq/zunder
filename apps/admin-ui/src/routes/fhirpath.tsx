import { createRoute } from "@tanstack/react-router";
import { FhirPathPlayground } from "@/components/FhirPathPlayground";
import { rootRoute } from "./root";

function FhirPathPage() {
  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <FhirPathPlayground />
    </div>
  );
}

export const fhirpathRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/fhirpath",
  component: FhirPathPage,
});
