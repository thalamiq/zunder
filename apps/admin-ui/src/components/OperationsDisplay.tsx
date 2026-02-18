import { useMemo, useState } from "react";
import type { OperationMetadata, OperationParameter } from "@/api/operations";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@thalamiq/ui/components/collapsible";
import { ChevronRight, Zap, Eye } from "lucide-react";

function ParameterRow({
  param,
  depth = 0,
}: {
  param: OperationParameter;
  depth?: number;
}) {
  return (
    <>
      <tr className="border-b border-border/50 text-sm">
        <td className="py-1.5 px-2 font-mono text-xs">
          <span style={{ paddingLeft: `${depth * 16}px` }}>{param.name}</span>
        </td>
        <td className="py-1.5 px-2">
          <Badge
            variant={param.use === "out" ? "secondary" : "outline"}
            className="text-xs"
          >
            {param.use}
          </Badge>
        </td>
        <td className="py-1.5 px-2 font-mono text-xs text-muted-foreground">
          {param.type ?? "-"}
        </td>
        <td className="py-1.5 px-2 font-mono text-xs text-muted-foreground">
          {param.min}..{param.max}
        </td>
        <td className="py-1.5 px-2 text-xs text-muted-foreground max-w-xs truncate">
          {param.documentation ?? ""}
        </td>
      </tr>
      {param.part?.map((child) => (
        <ParameterRow key={child.name} param={child} depth={depth + 1} />
      ))}
    </>
  );
}

function ParametersTable({ parameters }: { parameters: OperationParameter[] }) {
  const inParams = parameters.filter(
    (p) => p.use === "in" || p.use === "both"
  );
  const outParams = parameters.filter(
    (p) => p.use === "out" || p.use === "both"
  );

  return (
    <div className="space-y-3">
      {inParams.length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted-foreground mb-1">
            Input Parameters
          </p>
          <table className="w-full text-left">
            <thead>
              <tr className="border-b text-xs text-muted-foreground">
                <th className="py-1 px-2 font-medium">Name</th>
                <th className="py-1 px-2 font-medium">Use</th>
                <th className="py-1 px-2 font-medium">Type</th>
                <th className="py-1 px-2 font-medium">Card.</th>
                <th className="py-1 px-2 font-medium">Documentation</th>
              </tr>
            </thead>
            <tbody>
              {inParams.map((p) => (
                <ParameterRow key={p.name} param={p} />
              ))}
            </tbody>
          </table>
        </div>
      )}
      {outParams.length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted-foreground mb-1">
            Output Parameters
          </p>
          <table className="w-full text-left">
            <thead>
              <tr className="border-b text-xs text-muted-foreground">
                <th className="py-1 px-2 font-medium">Name</th>
                <th className="py-1 px-2 font-medium">Use</th>
                <th className="py-1 px-2 font-medium">Type</th>
                <th className="py-1 px-2 font-medium">Card.</th>
                <th className="py-1 px-2 font-medium">Documentation</th>
              </tr>
            </thead>
            <tbody>
              {outParams.map((p) => (
                <ParameterRow key={p.name} param={p} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function buildExampleEndpoint(op: OperationMetadata): string {
  if (op.system) {
    return `POST [base]/$${op.code}`;
  }
  if (op.type_level && op.type_contexts.length > 0) {
    return `POST [base]/${op.type_contexts[0]}/$${op.code}`;
  }
  if (op.instance && op.type_contexts.length > 0) {
    return `POST [base]/${op.type_contexts[0]}/[id]/$${op.code}`;
  }
  return `POST [base]/$${op.code}`;
}

function OperationCard({ operation }: { operation: OperationMetadata }) {
  const [open, setOpen] = useState(false);

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="font-mono text-base">${operation.code}</CardTitle>
          <div className="flex items-center gap-1.5">
            {operation.affects_state ? (
              <Badge variant="destructive" className="text-xs">
                <Zap className="h-3 w-3 mr-1" />
                Mutating
              </Badge>
            ) : (
              <Badge variant="secondary" className="text-xs">
                <Eye className="h-3 w-3 mr-1" />
                Read-only
              </Badge>
            )}
          </div>
        </div>
        <p className="text-sm text-muted-foreground">{operation.name}</p>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap gap-1.5">
          {operation.system && <Badge variant="outline">System</Badge>}
          {operation.type_level && <Badge variant="outline">Type</Badge>}
          {operation.instance && <Badge variant="outline">Instance</Badge>}
        </div>

        {operation.type_contexts.length > 0 && (
          <div>
            <p className="text-xs font-medium text-muted-foreground mb-1">
              Resource Types
            </p>
            <div className="flex flex-wrap gap-1">
              {operation.type_contexts.map((t) => (
                <Badge key={t} variant="secondary" className="text-xs">
                  {t}
                </Badge>
              ))}
            </div>
          </div>
        )}

        <div>
          <p className="text-xs font-medium text-muted-foreground mb-1">
            Example
          </p>
          <code className="text-xs bg-muted px-2 py-1 rounded block">
            {buildExampleEndpoint(operation)}
          </code>
        </div>

        {operation.parameters.length > 0 && (
          <Collapsible open={open} onOpenChange={setOpen}>
            <CollapsibleTrigger className="flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors">
              <ChevronRight
                className={`h-3 w-3 transition-transform ${open ? "rotate-90" : ""}`}
              />
              {operation.parameters.length} parameter
              {operation.parameters.length !== 1 ? "s" : ""}
            </CollapsibleTrigger>
            <CollapsibleContent className="mt-2">
              <ParametersTable parameters={operation.parameters} />
            </CollapsibleContent>
          </Collapsible>
        )}
      </CardContent>
    </Card>
  );
}

export function OperationsDisplay({
  operations,
}: {
  operations: OperationMetadata[];
}) {
  const sorted = useMemo(
    () => [...operations].sort((a, b) => a.code.localeCompare(b.code)),
    [operations]
  );

  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        {sorted.length} operation{sorted.length !== 1 ? "s" : ""} available
      </p>
      <div className="grid grid-cols-1 lg:grid-cols-2 2xl:grid-cols-3 gap-4">
        {sorted.map((op) => (
          <OperationCard key={op.code} operation={op} />
        ))}
      </div>
    </div>
  );
}
