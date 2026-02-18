import React from "react";
import {
  ArrowRightLeftIcon,
  BookOpenIcon,
  ClipboardListIcon,
  DatabaseIcon,
  FilterIcon,
  InfoIcon,
  LayersIcon,
  LayoutDashboardIcon,
  ScrollTextIcon,
  SettingsIcon,
  ZapIcon,
} from "lucide-react";
import { Package2Icon } from "lucide-react";

// When served from the same origin, no base URL needed
export const FHIR_SERVER_URL = window.location.origin;

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
    const response = await fetch("/admin/ui/config");
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

// Navigation types
export type LucideIcon = React.ComponentType<{ className?: string }>;

export interface NavSubItem {
  path: string;
  label: string;
}

export interface NavItem {
  path: string;
  label: string;
  icon: LucideIcon;
  subItems?: Record<string, NavSubItem>;
}

// Static navigation config
export const config: { nav: Record<string, NavItem> } = {
  nav: {
    dashboard: {
      path: "/dashboard",
      label: "Dashboard",
      icon: LayoutDashboardIcon,
    },
    api: {
      path: "/requests",
      label: "API",
      icon: ArrowRightLeftIcon,
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
    operations: {
      path: "/operations",
      label: "Operations",
      icon: ZapIcon,
    },
    terminology: {
      path: "/terminology",
      label: "Terminology",
      icon: BookOpenIcon,
    },
    jobs: {
      path: "/jobs",
      label: "Jobs",
      icon: ClipboardListIcon,
    },
    transactions: {
      path: "/transactions",
      label: "Transactions",
      icon: LayersIcon,
    },
    logs: {
      path: "/audit-logs",
      label: "Logs",
      icon: ScrollTextIcon,
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
