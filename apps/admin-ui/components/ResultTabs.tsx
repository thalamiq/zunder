import { RefObject, useEffect, useMemo, useRef, useState } from "react";
import { CustomTabsList, CustomTabsTrigger } from "@/components/CustomTabs";
import JsonViewer from "@/components/JsonViewer";
import BundleTableView from "./BundleTableView";
import ResourceTableView from "./ResourceTableView";
import JsonToolbar from "./JsonToolbar";
import { cn } from "@thalamiq/ui/utils";
import { useFullscreen, useToggle } from "react-use";
import { FhirResponse } from "@/lib/api/fhir";
import { Bundle, Resource } from "fhir/r4";

interface ResultTabsProps {
  data: FhirResponse;
  onNavigate?: (url: string) => void;
}

type TabValue = "json" | "table" | "json-rows";

const ResultTabs = ({ data, onNavigate }: ResultTabsProps) => {
  const ref = useRef<HTMLDivElement>(null);
  const [show, toggle] = useToggle(false);
  const [activeTab, setActiveTab] = useState<TabValue>("json");
  const isFullscreen = useFullscreen(ref as RefObject<Element>, show, {
    onClose: () => toggle(false),
  });

  // Check if data is a FHIR resource (has resourceType)
  const isFhirResource = useMemo(() => {
    return (
      typeof data === "object" &&
      data !== null &&
      "resourceType" in data &&
      typeof data.resourceType === "string"
    );
  }, [data]);

  // Extract bundle links if data is a Bundle
  const bundleLinks = useMemo(() => {
    if (
      isFhirResource &&
      data.resourceType === "Bundle" &&
      "link" in data &&
      Array.isArray((data as Bundle).link)
    ) {
      return (data as Bundle).link || [];
    }
    return [];
  }, [data, isFhirResource]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Escape to exit fullscreen
      if (e.key === "Escape" && isFullscreen) {
        toggle();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isFullscreen, toggle]);

  return (
    <div
      ref={ref}
      className={cn(
        "flex flex-col overflow-hidden bg-card transition-all duration-300",
        isFullscreen && "fixed inset-0 z-50 bg-card shadow-2xl"
      )}
    >
      {/* Header with Tabs and Toolbar */}
      <JsonToolbar
        data={data}
        isFullscreen={isFullscreen}
        toggleFullscreen={toggle}
        bundleLinks={bundleLinks}
        onNavigate={onNavigate}
        tabsSlot={
          <CustomTabsList>
            <CustomTabsTrigger
              value="json"
              active={activeTab === "json"}
              onClick={() => setActiveTab("json")}
            >
              Raw
            </CustomTabsTrigger>
            {isFhirResource && (
              <>
                <CustomTabsTrigger
                  value="table"
                  active={activeTab === "table"}
                  onClick={() => setActiveTab("table")}
                >
                  Table
                </CustomTabsTrigger>
                <CustomTabsTrigger
                  value="json-rows"
                  active={activeTab === "json-rows"}
                  onClick={() => setActiveTab("json-rows")}
                >
                  JSON Rows
                </CustomTabsTrigger>
              </>
            )}
          </CustomTabsList>
        }
      />

      {/* Tab Content */}
      <div className="flex-1 overflow-auto">
        {activeTab === "json" && <JsonViewer data={data} />}
        {activeTab === "json-rows" && isFhirResource && (
          <BundleTableView bundle={data as Resource} />
        )}
        {activeTab === "table" && isFhirResource && (
          <ResourceTableView data={data} />
        )}
      </div>
    </div>
  );
};

export default ResultTabs;
