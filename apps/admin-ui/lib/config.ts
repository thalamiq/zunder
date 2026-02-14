import {
  ClipboardListIcon,
  DatabaseIcon,
  FileTextIcon,
  FilterIcon,
  InfoIcon,
  LayoutDashboardIcon,
  SendIcon,
  SettingsIcon,
} from "lucide-react";
import { Package2Icon } from "lucide-react";

// FHIR server configuration
export const FHIR_SERVER_URL =
  process.env.NEXT_PUBLIC_FHIR_SERVER_URL ||
  process.env.FHIR_SERVER_URL ||
  "http://localhost:8080";

// UI Configuration from server
export interface UiConfig {
  enabled: boolean;
  title: string;
  requires_auth: boolean;
  runtime_config_enabled: boolean;
}

// Cache for UI config
let cachedConfig: UiConfig | null = null;

/**
 * Fetch UI configuration from the server
 */
export async function fetchUiConfig(): Promise<UiConfig> {
  if (cachedConfig) {
    return cachedConfig;
  }

  try {
    const response = await fetch("/api/admin/ui/config");
    if (!response.ok) {
      throw new Error(`Failed to fetch UI config: ${response.statusText}`);
    }
    cachedConfig = await response.json();
    return cachedConfig!;
  } catch (error) {
    console.error("Failed to fetch UI config, using defaults:", error);
    // Return default config if fetch fails
    cachedConfig = {
      enabled: true,
      title: "FHIR Server Admin",
      requires_auth: false,
      runtime_config_enabled: true,
    };
    return cachedConfig;
  }
}

/**
 * Clear cached config (useful after logout)
 */
export function clearCachedConfig() {
  cachedConfig = null;
}

// Static navigation config
export const config = {
  nav: {
    dashboard: {
      path: "/dashboard",
      label: "Dashboard",
      icon: LayoutDashboardIcon,
    },
    api: {
      path: "/requests",
      label: "API",
      icon: SendIcon,
    },
    resources: {
      path: "/resources",
      label: "Resources",
      icon: DatabaseIcon,
    },
    search: {
      path: "/search",
      label: "Search",
      icon: FilterIcon,
      subItems: {
        searchParameters: {
          path: "/search/search-parameters",
          label: "Parameters",
        },
        compartments: {
          path: "/search/compartments",
          label: "Compartments",
        },
        coverage: {
          path: "/search/search-coverage",
          label: "Coverage",
        },
        indexTables: {
          path: "/search/index-tables",
          label: "Index Tables",
        },
      },
    },
    packages: {
      path: "/packages",
      label: "Packages",
      icon: Package2Icon,
    },
    jobs: {
      path: "/jobs",
      label: "Jobs",
      icon: ClipboardListIcon,
    },
    logs: {
      path: "/audit-logs",
      label: "Logs",
      icon: FileTextIcon,
    },
    metadata: {
      path: "/metadata",
      label: "Metadata",
      icon: InfoIcon,
    },
    settings: {
      path: "/settings",
      label: "Settings",
      icon: SettingsIcon,
    },
  },
};
