import { lazy, RefObject, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { CustomTabsList, CustomTabsTrigger } from "@/components/CustomTabs";
import JsonViewer from "@/components/JsonViewer";
import BundleTableView from "./BundleTableView";
import ResourceTableView from "./ResourceTableView";
import JsonToolbar from "./JsonToolbar";
import { cn } from "@thalamiq/ui/utils";
import { useFullscreen, useToggle } from "react-use";
import { FhirResponse } from "@/api/fhir";
import { Bundle, Resource } from "fhir/r4";

const ReferenceGraph = lazy(() => import("./ReferenceGraph"));

interface ResultTabsProps {
  data: FhirResponse;
  onNavigate?: (url: string) => void;
}

type TabValue = "json" | "table" | "json-rows" | "graph";

const ResultTabs = ({ data, onNavigate }: ResultTabsProps) => {
  const ref = useRef<HTMLDivElement>(null);
  const [show, toggle] = useToggle(false);
  const [activeTab, setActiveTab] = useState<TabValue>("json");
  const isFullscreen = useFullscreen(ref as RefObject<Element>, show, {
    onClose: () => toggle(false),
  });

  const isFhirResource = useMemo(() => {
    return (
      typeof data === "object" &&
      data !== null &&
      "resourceType" in data &&
      typeof data.resourceType === "string"
    );
  }, [data]);

  const isBundle = useMemo(
    () => isFhirResource && (data as Resource).resourceType === "Bundle",
    [data, isFhirResource],
  );

  const resourceMeta = useMemo(() => {
    if (
      isFhirResource &&
      !isBundle &&
      "id" in data &&
      typeof (data as Resource).id === "string"
    ) {
      return {
        resourceType: (data as Resource).resourceType!,
        resourceId: (data as Resource).id!,
      };
    }
    return null;
  }, [data, isFhirResource, isBundle]);

  const showGraphTab = !!resourceMeta || isBundle;

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

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isFullscreen) toggle();
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
                {showGraphTab && (
                  <CustomTabsTrigger
                    value="graph"
                    active={activeTab === "graph"}
                    onClick={() => setActiveTab("graph")}
                  >
                    Graph
                  </CustomTabsTrigger>
                )}
              </>
            )}
          </CustomTabsList>
        }
      />

      <div className="flex-1 overflow-auto">
        {activeTab === "json" && <JsonViewer data={data} />}
        {activeTab === "json-rows" && isFhirResource && (
          <BundleTableView bundle={data as Resource} />
        )}
        {activeTab === "table" && isFhirResource && (
          <ResourceTableView data={data} />
        )}
        {activeTab === "graph" && showGraphTab && (
          <Suspense
            fallback={
              <div className="flex items-center justify-center h-64 text-muted-foreground text-sm">
                Loading graph&hellip;
              </div>
            }
          >
            {isBundle ? (
              <ReferenceGraph
                bundle={data as unknown as Record<string, unknown>}
                onNavigate={onNavigate}
              />
            ) : resourceMeta ? (
              <ReferenceGraph
                resourceType={resourceMeta.resourceType}
                resourceId={resourceMeta.resourceId}
                onNavigate={onNavigate}
              />
            ) : null}
          </Suspense>
        )}
      </div>
    </div>
  );
};

export default ResultTabs;
