import { createRouter } from "@tanstack/react-router";
import { rootRoute } from "./root";
import { indexRoute } from "./index";
import { loginRoute } from "./login";
import { dashboardRoute } from "./dashboard";
import { resourcesRoute } from "./resources";
import { requestsRoute } from "./requests";
import { metadataRoute } from "./metadata";
import { jobsRoute } from "./jobs";
import { settingsRoute } from "./settings";
import { auditLogsRoute } from "./audit-logs";
import { transactionsRoute } from "./transactions";
import { packagesRoute } from "./packages";
import { packageDetailRoute } from "./package-detail";
import { searchRoute } from "./search/search";
import { searchParametersRoute } from "./search/search-parameters";
import { compartmentsRoute } from "./search/compartments";
import { searchCoverageRoute } from "./search/search-coverage";
import { indexTablesRoute } from "./search/index-tables";
import { operationsRoute } from "./operations";
import { terminologyRoute } from "./terminology";
import { fhirpathRoute } from "./fhirpath";

const routeTree = rootRoute.addChildren([
  indexRoute,
  loginRoute,
  dashboardRoute,
  resourcesRoute,
  requestsRoute,
  metadataRoute,
  jobsRoute,
  settingsRoute,
  auditLogsRoute,
  transactionsRoute,
  packagesRoute,
  packageDetailRoute,
  operationsRoute,
  terminologyRoute,
  fhirpathRoute,
  searchRoute,
  searchParametersRoute,
  compartmentsRoute,
  searchCoverageRoute,
  indexTablesRoute,
]);

export const router = createRouter({
  routeTree,
  basepath: "/ui",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
