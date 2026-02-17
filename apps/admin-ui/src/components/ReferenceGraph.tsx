import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type NodeTypes,
  Handle,
  Position,
  type NodeMouseHandler,
} from "@xyflow/react";
import ELK from "elkjs/lib/elk.bundled.js";
import "@xyflow/react/dist/style.css";
import { fetchResourceReferences, type ReferenceEdge } from "@/api/references";
import { fetchFhir } from "@/api/fhir";
import { queryKeys } from "@/api/query-keys";
import { ChevronRight, X } from "lucide-react";
import { cn } from "@thalamiq/ui/utils";
import JsonViewer from "./JsonViewer";

// --- Colors ---
const RESOURCE_COLORS: Record<string, string> = {
  Patient: "#3b82f6",
  Encounter: "#22c55e",
  Observation: "#f59e0b",
  Condition: "#ef4444",
  MedicationRequest: "#8b5cf6",
  Medication: "#a855f7",
  Practitioner: "#06b6d4",
  Organization: "#14b8a6",
  Location: "#10b981",
  DiagnosticReport: "#f97316",
  Procedure: "#ec4899",
  AllergyIntolerance: "#e11d48",
  Immunization: "#6366f1",
  CarePlan: "#0ea5e9",
  Claim: "#84cc16",
  Coverage: "#eab308",
  ServiceRequest: "#d946ef",
  Specimen: "#78716c",
  DocumentReference: "#64748b",
};

const getColor = (rt: string): string => RESOURCE_COLORS[rt] ?? "#6b7280";

// --- Node ---
interface ResourceNodeData {
  resourceType: string;
  resourceId: string;
  isFocal: boolean;
  selected?: boolean;
  [key: string]: unknown;
}

function ResourceNode({ data }: { data: ResourceNodeData }) {
  const color = getColor(data.resourceType);
  const label =
    data.resourceId.length > 12
      ? data.resourceId.slice(0, 12) + "\u2026"
      : data.resourceId;

  return (
    <div
      className="bg-card rounded-md px-3 py-1.5 min-w-[110px] text-center transition-shadow"
      style={{
        border: `${data.isFocal || data.selected ? 2 : 1}px solid ${color}`,
        boxShadow: data.selected
          ? `0 0 0 3px ${color}44`
          : data.isFocal
            ? `0 0 0 2px ${color}22`
            : undefined,
      }}
    >
      <Handle type="target" position={Position.Left} className="!bg-muted-foreground/60 !w-1.5 !h-1.5 !border-0" />
      <div className="text-[11px] font-medium leading-tight" style={{ color }}>
        {data.resourceType}
      </div>
      <div className="text-[10px] text-muted-foreground font-mono leading-tight">
        {label}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-muted-foreground/60 !w-1.5 !h-1.5 !border-0" />
    </div>
  );
}

const nodeTypes: NodeTypes = { resource: ResourceNode };

// --- ELK layout ---
const elk = new ELK();

async function layoutGraph(nodes: Node[], edges: Edge[]): Promise<Node[]> {
  const graph = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.layered.spacing.nodeNodeBetweenLayers": "80",
      "elk.spacing.nodeNode": "50",
    },
    children: nodes.map((n) => ({ id: n.id, width: 140, height: 44 })),
    edges: edges.map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
  };
  const laid = await elk.layout(graph);
  const posMap = new Map(
    (laid.children ?? []).map((n) => [n.id, { x: n.x ?? 0, y: n.y ?? 0 }]),
  );
  return nodes.map((n) => ({ ...n, position: posMap.get(n.id) ?? { x: 0, y: 0 } }));
}

// --- Build ReactFlow data ---
function buildGraph(
  refEdges: ReferenceEdge[],
  focalType: string,
  focalId: string,
): { nodes: Node[]; edges: Edge[] } {
  const nodeMap = new Map<string, ResourceNodeData>();
  const add = (t: string, id: string) => {
    const k = `${t}/${id}`;
    if (!nodeMap.has(k))
      nodeMap.set(k, { resourceType: t, resourceId: id, isFocal: t === focalType && id === focalId });
  };
  add(focalType, focalId);

  const flowEdges: Edge[] = [];
  const seen = new Set<string>();
  for (const e of refEdges) {
    add(e.sourceType, e.sourceId);
    add(e.targetType, e.targetId);
    const k = `${e.sourceType}/${e.sourceId}->${e.targetType}/${e.targetId}:${e.parameterName}`;
    if (seen.has(k)) continue;
    seen.add(k);
    flowEdges.push({
      id: k,
      source: `${e.sourceType}/${e.sourceId}`,
      target: `${e.targetType}/${e.targetId}`,
      label: e.parameterName,
      type: "default",
      style: { stroke: "#94a3b8", strokeWidth: 1 },
      labelStyle: { fontSize: 9, fill: "#94a3b8" },
      labelBgStyle: { fill: "var(--card)", fillOpacity: 0.9 },
      markerEnd: { type: "arrowclosed" as const, color: "#94a3b8" },
    });
  }

  return {
    nodes: Array.from(nodeMap.entries()).map(([k, d]) => ({
      id: k, type: "resource", position: { x: 0, y: 0 }, data: d,
    })),
    edges: flowEdges,
  };
}

// --- Detail panel (right side) ---
function DetailPanel({
  resourceType,
  resourceId,
  onClose,
}: {
  resourceType: string;
  resourceId: string;
  onClose: () => void;
}) {
  const { data, isLoading, isError } = useQuery({
    queryKey: queryKeys.fhir(`${resourceType}/${resourceId}`),
    queryFn: () => fetchFhir(`${resourceType}/${resourceId}`),
  });

  const color = getColor(resourceType);

  return (
    <div className="w-[360px] shrink-0 border-l bg-card flex flex-col overflow-hidden">
      <div className="flex items-center justify-between px-3 py-2 border-b">
        <div className="flex items-center gap-2 min-w-0">
          <span className="inline-block w-2 h-2 rounded-sm shrink-0" style={{ backgroundColor: color }} />
          <span className="text-xs font-medium truncate" style={{ color }}>
            {resourceType}
          </span>
          <span className="text-xs text-muted-foreground font-mono truncate">
            {resourceId}
          </span>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="p-1 rounded hover:bg-muted transition-colors shrink-0"
        >
          <X className="h-3.5 w-3.5 text-muted-foreground" />
        </button>
      </div>
      <div className="flex-1 overflow-auto p-3">
        {isLoading && (
          <div className="text-xs text-muted-foreground">Loading&hellip;</div>
        )}
        {isError && (
          <div className="text-xs text-destructive">Failed to load resource.</div>
        )}
        {data && (
          <JsonViewer data={data} className="rounded-md" />
        )}
      </div>
    </div>
  );
}

// ============================================================
// Single-resource graph
// ============================================================
function SingleResourceGraph({
  resourceType,
  resourceId,
  onNavigate,
  className,
}: {
  resourceType: string;
  resourceId: string;
  onNavigate?: (url: string) => void;
  className?: string;
}) {
  const [selected, setSelected] = useState<{ type: string; id: string } | null>(null);

  const { data: apiData, isLoading, isError, error } = useQuery({
    queryKey: queryKeys.resourceReferences(resourceType, resourceId),
    queryFn: () => fetchResourceReferences(resourceType, resourceId),
  });

  const graphData = useMemo(() => {
    if (!apiData) return null;
    const all = [...apiData.outgoing, ...apiData.incoming];
    if (all.length === 0) return null;
    return buildGraph(all, resourceType, resourceId);
  }, [apiData, resourceType, resourceId]);

  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);

  // Layout on data change
  useEffect(() => {
    if (!graphData) return;
    setEdges(graphData.edges);
    layoutGraph(graphData.nodes, graphData.edges).then(setNodes);
  }, [graphData, setNodes, setEdges]);

  // Update selected highlight on nodes
  useEffect(() => {
    setNodes((nds) =>
      nds.map((n) => {
        const d = n.data as ResourceNodeData;
        const isSelected = selected?.type === d.resourceType && selected?.id === d.resourceId;
        if (d.selected === isSelected) return n;
        return { ...n, data: { ...d, selected: isSelected } };
      }),
    );
  }, [selected, setNodes]);

  const onNodeClick: NodeMouseHandler<Node> = useCallback((_event, node) => {
    const d = node.data as ResourceNodeData;
    setSelected((prev) =>
      prev?.type === d.resourceType && prev?.id === d.resourceId
        ? null
        : { type: d.resourceType, id: d.resourceId },
    );
  }, []);

  const onNodeDoubleClick: NodeMouseHandler<Node> = useCallback(
    (_event, node) => {
      const d = node.data as ResourceNodeData;
      onNavigate?.(`${d.resourceType}/${d.resourceId}`);
    },
    [onNavigate],
  );

  const onPaneClick = useCallback(() => setSelected(null), []);

  if (isLoading)
    return <div className={cn("flex items-center justify-center text-muted-foreground text-sm", className)}>Loading&hellip;</div>;
  if (isError)
    return <div className={cn("flex items-center justify-center text-destructive text-sm", className)}>Error: {error?.message}</div>;
  if (!graphData || graphData.edges.length === 0)
    return <div className={cn("flex items-center justify-center text-muted-foreground text-sm", className)}>No references found.</div>;

  return (
    <div className={cn("flex", className)}>
      <div className="flex-1 min-w-0">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          nodeTypes={nodeTypes}
          onNodeClick={onNodeClick}
          onNodeDoubleClick={onNodeDoubleClick}
          onPaneClick={onPaneClick}
          fitView
          fitViewOptions={{ padding: 0.2 }}
          minZoom={0.2}
          maxZoom={2}
          proOptions={{ hideAttribution: true }}
        >
          <Background gap={16} size={0.5} />
          <Controls showInteractive={false} />
          <MiniMap
            pannable
            zoomable
            nodeColor={(n) => getColor((n.data as ResourceNodeData).resourceType)}
            maskColor="rgba(0,0,0,0.08)"
            className="!border !rounded-md"
          />
        </ReactFlow>
      </div>
      {selected && (
        <DetailPanel
          resourceType={selected.type}
          resourceId={selected.id}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}

// ============================================================
// Bundle accordion item
// ============================================================
function BundleEntryItem({
  resourceType,
  resourceId,
  onNavigate,
}: {
  resourceType: string;
  resourceId: string;
  onNavigate?: (url: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const color = getColor(resourceType);

  return (
    <div className="border-b last:border-b-0">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="w-full flex items-center gap-2 px-4 py-2 text-left hover:bg-muted/50 transition-colors"
      >
        <ChevronRight
          className={cn(
            "h-3.5 w-3.5 text-muted-foreground shrink-0 transition-transform",
            open && "rotate-90",
          )}
        />
        <span className="inline-block w-2 h-2 rounded-sm shrink-0" style={{ backgroundColor: color }} />
        <span className="text-xs font-medium" style={{ color }}>{resourceType}</span>
        <span className="text-xs text-muted-foreground font-mono truncate">{resourceId}</span>
      </button>
      {open && (
        <div className="px-4 pb-3">
          <SingleResourceGraph
            resourceType={resourceType}
            resourceId={resourceId}
            onNavigate={onNavigate}
            className="h-[calc(100vh-150px)]"
          />
        </div>
      )}
    </div>
  );
}

// ============================================================
// Exported component
// ============================================================
interface ReferenceGraphProps {
  resourceType?: string;
  resourceId?: string;
  bundle?: Record<string, unknown>;
  onNavigate?: (url: string) => void;
}

export default function ReferenceGraph({
  resourceType,
  resourceId,
  bundle,
  onNavigate,
}: ReferenceGraphProps) {
  if (resourceType && resourceId && !bundle) {
    return (
      <SingleResourceGraph
        resourceType={resourceType}
        resourceId={resourceId}
        onNavigate={onNavigate}
        className="h-[calc(100vh-150px)]"
      />
    );
  }

  if (bundle) {
    const entries = (bundle as { entry?: { resource?: Record<string, unknown> }[] }).entry;
    if (!Array.isArray(entries)) return null;

    const items: { resourceType: string; resourceId: string }[] = [];
    const seen = new Set<string>();
    for (const e of entries) {
      const r = e.resource;
      if (!r?.resourceType || !r?.id) continue;
      const key = `${r.resourceType}/${r.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      items.push({ resourceType: r.resourceType as string, resourceId: r.id as string });
    }

    if (items.length === 0) return null;

    return (
      <div className="border rounded-md overflow-hidden">
        {items.map((item) => (
          <BundleEntryItem
            key={`${item.resourceType}/${item.resourceId}`}
            resourceType={item.resourceType}
            resourceId={item.resourceId}
            onNavigate={onNavigate}
          />
        ))}
      </div>
    );
  }

  return null;
}
