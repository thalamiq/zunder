import { useState, useCallback } from "react";
import { Button } from "@thalamiq/ui/components/button";
import { Input } from "@thalamiq/ui/components/input";
import { Label } from "@thalamiq/ui/components/label";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import { PlayIcon, WandSparklesIcon, BracesIcon } from "lucide-react";
import { PageHeader } from "./PageHeader";
import { evaluateFhirPath, type FhirPathResponse } from "@/api/fhirpath";
import JsonViewer from "./JsonViewer";

const SAMPLE_PATIENT = JSON.stringify(
  {
    resourceType: "Patient",
    id: "example",
    name: [
      {
        use: "official",
        family: "Smith",
        given: ["John", "Jacob"],
      },
    ],
    gender: "male",
    birthDate: "1990-01-01",
    address: [
      {
        use: "home",
        city: "Springfield",
        state: "IL",
      },
    ],
  },
  null,
  2,
);

export function FhirPathPlayground() {
  const [expression, setExpression] = useState("name.family");
  const [resourceText, setResourceText] = useState(SAMPLE_PATIENT);
  const [result, setResult] = useState<FhirPathResponse | null>(null);
  const [error, setError] = useState<{
    label: string;
    detail: string;
  } | null>(null);
  const [loading, setLoading] = useState(false);

  const handleEvaluate = useCallback(async () => {
    setError(null);
    setResult(null);

    let resource: object;
    try {
      resource = JSON.parse(resourceText);
    } catch (e) {
      setError({
        label: "Invalid JSON",
        detail:
          e instanceof SyntaxError ? e.message : "Could not parse the resource.",
      });
      return;
    }

    if (!expression.trim()) {
      setError({ label: "Missing expression", detail: "Please enter a FHIRPath expression." });
      return;
    }

    setLoading(true);
    try {
      const resp = await evaluateFhirPath({ expression, resource });
      setResult(resp);
    } catch (e) {
      setError({
        label: "Expression error",
        detail: e instanceof Error ? e.message : "Evaluation failed.",
      });
    } finally {
      setLoading(false);
    }
  }, [expression, resourceText]);

  const handleExpressionKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleEvaluate();
      }
    },
    [handleEvaluate],
  );

  const handleResourceKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
        e.preventDefault();
        handleEvaluate();
      }
    },
    [handleEvaluate],
  );

  const handleFormat = useCallback(() => {
    try {
      const parsed = JSON.parse(resourceText);
      setResourceText(JSON.stringify(parsed, null, 2));
    } catch {
      // ignore formatting errors
    }
  }, [resourceText]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-hidden p-6">
      <PageHeader title="FHIRPath Playground" />

      <div className="flex shrink-0 items-end gap-2">
        <div className="flex-1">
          <Label htmlFor="fhirpath-expression">Expression</Label>
          <Input
            id="fhirpath-expression"
            value={expression}
            onChange={(e) => setExpression(e.target.value)}
            onKeyDown={handleExpressionKeyDown}
            placeholder="Patient.name.family"
            className="font-mono"
          />
        </div>
        <Button onClick={handleEvaluate} disabled={loading}>
          <PlayIcon className="mr-1.5 h-4 w-4" />
          {loading ? "Evaluating..." : "Evaluate"}
        </Button>
      </div>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-hidden lg:flex-row">
        <Card className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <CardHeader className="shrink-0 pb-2">
            <div className="flex items-center justify-between">
              <CardTitle className="text-sm font-medium">Resource</CardTitle>
              <div className="flex gap-1">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleFormat}
                  title="Format JSON"
                >
                  <BracesIcon className="mr-1 h-3.5 w-3.5" />
                  Format
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setResourceText(SAMPLE_PATIENT)}
                  title="Load sample Patient"
                >
                  <WandSparklesIcon className="mr-1 h-3.5 w-3.5" />
                  Sample
                </Button>
              </div>
            </div>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col overflow-hidden">
            <textarea
              value={resourceText}
              onChange={(e) => setResourceText(e.target.value)}
              onKeyDown={handleResourceKeyDown}
              className="min-h-0 w-full flex-1 resize-none overflow-auto rounded-md border bg-muted/50 p-3 font-mono text-sm focus:outline-none focus:ring-1 focus:ring-ring"
              spellCheck={false}
            />
          </CardContent>
        </Card>

        <Card className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <CardHeader className="shrink-0 pb-2">
            <div className="flex items-center justify-between">
              <CardTitle className="text-sm font-medium">Result</CardTitle>
              {result && (
                <div className="flex items-center gap-2">
                  <Badge variant="secondary">
                    {result.count} {result.count === 1 ? "value" : "values"}
                  </Badge>
                  <Badge variant="outline">
                    {result.elapsed_ms.toFixed(2)} ms
                  </Badge>
                </div>
              )}
            </div>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col overflow-hidden">
            {error && (
              <div className="shrink-0 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
                <span className="font-medium">{error.label}: </span>
                <span className="font-mono">{error.detail}</span>
              </div>
            )}
            {result && !error && (
              <div className="min-h-0 flex-1 overflow-auto">
                <JsonViewer
                  data={result.result.length === 1 ? result.result[0] : result.result}
                />
              </div>
            )}
            {!result && !error && (
              <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">
                Press Evaluate, Enter, or Ctrl+Enter to run
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
