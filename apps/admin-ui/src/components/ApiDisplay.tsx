import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { useNavigate, useLocation } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@thalamiq/ui/components/select";
import { Button } from "@thalamiq/ui/components/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@thalamiq/ui/components/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@thalamiq/ui/components/dialog";
import { Input } from "@thalamiq/ui/components/input";
import { Label } from "@thalamiq/ui/components/label";
import { Badge } from "@thalamiq/ui/components/badge";
import { Code, X, Settings2, Send } from "lucide-react";
import { fetchMetadata } from "@/api/metadata";
import { queryKeys } from "@/api/query-keys";
import { FhirResponse } from "@/api/fhir";
import FhirInput from "./FhirInput";
import StatusBadge from "./StatusBadge";
import ResultTabs from "./ResultTabs";
import { ErrorArea } from "@/components/Error";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@thalamiq/ui/components/empty";
import { cn } from "@thalamiq/ui/utils";
import { PageHeader } from "./PageHeader";

type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

type ResponseStatus = {
  status: number;
  statusText: string;
  data: unknown;
  headers: Record<string, string>;
  error?: string;
};

type CustomHeader = {
  id: string;
  key: string;
  value: string;
};

const METHODS_WITH_BODY: HttpMethod[] = ["POST", "PUT", "PATCH"];
const VALID_METHODS: HttpMethod[] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

const getValidMethod = (method: string | null): HttpMethod => {
  if (method && VALID_METHODS.includes(method as HttpMethod)) {
    return method as HttpMethod;
  }
  return "GET";
};

export default function ApiDisplay() {
  const navigate = useNavigate();
  const location = useLocation();
  const pathname = location.pathname;
  const searchParams = new URLSearchParams(location.search);
  const endpointParam = searchParams.get("endpoint") || "";
  const queriedMethod = getValidMethod(searchParams.get("method"));

  // State
  const [httpMethod, setHttpMethod] = useState<HttpMethod>(() => queriedMethod);
  const [endpoint, setEndpoint] = useState(endpointParam);
  const [requestBody, setRequestBody] = useState("");
  const [loading, setLoading] = useState(false);
  const [response, setResponse] = useState<ResponseStatus | null>(null);
  const [customHeaders, setCustomHeaders] = useState<CustomHeader[]>([]);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [headerKey, setHeaderKey] = useState("");
  const [headerValue, setHeaderValue] = useState("");
  const [responseHeadersDialogOpen, setResponseHeadersDialogOpen] =
    useState(false);

  // Queries
  const metadataQuery = useQuery({
    queryKey: queryKeys.metadata("full"),
    queryFn: () => fetchMetadata({ mode: "full" }),
  });

  // Memos
  const resourceType = useMemo(() => {
    if (!endpoint.trim()) return null;
    const resourcePart = endpoint.split("?")[0];
    const match = resourcePart.match(/^([A-Z][a-zA-Z]*)/);
    return match ? match[1] : null;
  }, [endpoint]);

  const requiresBody = METHODS_WITH_BODY.includes(httpMethod);

  // Handlers
  const handleApiRequest = useCallback(async () => {
    // Get endpoint and method from state or URL params
    const methodToUse = httpMethod || queriedMethod;
    const requiresBodyForMethod = METHODS_WITH_BODY.includes(methodToUse);

    // For methods with body (POST, PUT, PATCH), always use current input as endpoint
    // For GET/DELETE, use current input or fallback to URL param
    const endpointToUse = requiresBodyForMethod
      ? endpoint.trim()
      : endpoint.trim() || endpointParam;

    const currentUrlEndpoint = endpointParam;
    const currentUrlMethod = queriedMethod;

    // Validate request body
    if (requiresBodyForMethod) {
      if (!requestBody.trim()) {
        toast.error("Please enter a request body");
        return;
      }
      try {
        JSON.parse(requestBody);
      } catch {
        toast.error("Invalid JSON in request body");
        return;
      }
    }

    // Confirm DELETE
    if (methodToUse === "DELETE" && !confirm(`Delete ${endpointToUse}?`)) {
      return;
    }

    // Update URL with endpoint and method
    const params = new URLSearchParams(searchParams.toString());

    // Only update URL if values have changed
    const endpointChanged = endpointToUse !== currentUrlEndpoint;
    const methodChanged = methodToUse !== currentUrlMethod;

    if (endpointChanged || methodChanged) {
      if (endpointToUse) {
        params.set("endpoint", endpointToUse);
      } else {
        params.delete("endpoint");
      }
      params.set("method", methodToUse);
      const newUrl = params.toString() ? `${pathname}?${params}` : pathname;
      navigate({ to: newUrl });
    }

    setLoading(true);

    try {
      const headers: Record<string, string> = {
        Accept: "application/fhir+json",
      };

      if (requiresBodyForMethod) {
        headers["Content-Type"] = "application/fhir+json";
      }

      // Add custom headers
      customHeaders.forEach(({ key, value }) => {
        if (key.trim() && value.trim()) {
          headers[key.trim()] = value.trim();
        }
      });

      const options: RequestInit = { method: methodToUse, headers };
      if (requiresBodyForMethod && requestBody) {
        options.body = requestBody;
      }

      // Use endpointToUse which has fallback to URL params
      const res = await fetch(`/fhir/${endpointToUse}`, options);
      const contentType = res.headers.get("content-type");

      let data = null;

      // Try to parse JSON for any JSON content type (including application/json and application/fhir+json)
      // Also try to parse for error responses (4xx/5xx) as they may contain OperationOutcome
      const isJsonContentType =
        contentType?.includes("json") ||
        contentType?.includes("application/fhir+json");
      const isErrorStatus = res.status >= 400;

      if (isJsonContentType || isErrorStatus) {
        try {
          const text = await res.text();
          if (text.trim()) {
            try {
              data = JSON.parse(text);

              // Check for too-costly error and provide helpful message
              if (
                data?.resourceType === "OperationOutcome" &&
                data?.issue?.[0]?.code === "too-costly"
              ) {
                const diagnostics = data.issue[0].diagnostics || "";
                toast.error("Request Too Costly", {
                  description: diagnostics,
                  duration: 10000,
                });
              }
            } catch {
              // If JSON parsing fails, use text as message
              data = { message: text };
            }
          } else {
            data = { message: `Response: ${res.status} ${res.statusText}` };
          }
        } catch {
          data = { message: `Response: ${res.status} ${res.statusText}` };
        }
      } else {
        // For non-JSON responses, try to read as text
        try {
          const text = await res.text();
          if (text.trim()) {
            data = { message: text };
          } else {
            data = { message: `Response: ${res.status} ${res.statusText}` };
          }
        } catch {
          data = { message: `Response: ${res.status} ${res.statusText}` };
        }
      }

      // Capture response headers
      const responseHeaders: Record<string, string> = {};
      res.headers.forEach((value, key) => {
        responseHeaders[key] = value;
      });

      setResponse({
        status: res.status,
        statusText: res.statusText,
        data,
        headers: responseHeaders,
      });
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : "Request failed";
      setResponse({
        status: 0,
        statusText: "Error",
        data: null,
        headers: {},
        error: errorMessage,
      });
      toast.error(errorMessage);
    } finally {
      setLoading(false);
    }
  }, [
    endpoint,
    requestBody,
    httpMethod,
    requiresBody,
    navigate,
    pathname,
    location.search,
    customHeaders,
    endpointParam,
    queriedMethod,
  ]);

  // Track previous endpoint/method to detect changes
  const previousParams = useRef<{ endpoint: string; method: HttpMethod }>({
    endpoint: "",
    method: "GET",
  });

  // Keep a ref to the latest handleApiRequest function
  const handleApiRequestRef = useRef(handleApiRequest);
  useEffect(() => {
    handleApiRequestRef.current = handleApiRequest;
  });

  // Sync endpoint and method from URL
  useEffect(() => {
    setEndpoint((current: string) =>
      endpointParam !== current ? endpointParam : current
    );
    setHttpMethod((current) =>
      queriedMethod !== current ? queriedMethod : current
    );
  }, [endpointParam, queriedMethod]);

  // Auto-execute request when endpoint/method changes in URL (only for GET requests)
  useEffect(() => {
    if (!endpointParam) {
      previousParams.current = { endpoint: "", method: "GET" };
      return;
    }

    const hasChanged =
      endpointParam !== previousParams.current.endpoint ||
      queriedMethod !== previousParams.current.method;

    if (hasChanged) {
      previousParams.current = {
        endpoint: endpointParam,
        method: queriedMethod,
      };
      // Only auto-execute GET requests
      if (queriedMethod === "GET") {
        // Set loading immediately to prevent empty state blink
        setLoading(true);
        setTimeout(() => {
          handleApiRequestRef.current();
        }, 100);
      }
    }
  }, [endpointParam, queriedMethod]);

  // Keyboard shortcut: Cmd/Ctrl + Enter
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && !loading) {
        e.preventDefault();
        handleApiRequest();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [loading, endpoint, requestBody, httpMethod, handleApiRequest]);

  const handleSearch = useCallback(
    (e: React.FormEvent<Element>) => {
      e.preventDefault();
      handleApiRequest();
    },
    [handleApiRequest]
  );

  const handleMethodChange = (method: HttpMethod) => {
    setResponse(null);
    setHttpMethod(method);

    // Update URL with new method, preserving existing params
    const params = new URLSearchParams(searchParams.toString());
    params.set("method", method);
    // Preserve endpoint from URL params (don't rely on state which might not be synced)
    const urlEndpoint = searchParams.get("endpoint");
    if (urlEndpoint) {
      params.set("endpoint", urlEndpoint);
    } else if (endpoint.trim()) {
      // Fallback to state if URL doesn't have it but state does
      params.set("endpoint", endpoint);
    }
    const newUrl = params.toString() ? `${pathname}?${params}` : pathname;
    navigate({ to: newUrl });
  };

  const handleFormatJson = () => {
    if (!requestBody.trim()) return;

    try {
      const parsed = JSON.parse(requestBody);
      setRequestBody(JSON.stringify(parsed, null, 2));
    } catch (e) {
      const message = e instanceof Error ? e.message : "Unknown error";
      toast.error(`Invalid JSON: ${message}`, {
        description: "Check your JSON syntax",
        duration: 5000,
      });
    }
  };

  const handleBundleNavigation = useCallback(
    (url: string) => {
      try {
        // Extract the path from the bundle link URL
        // URLs are like: http://localhost:8080/fhir/CodeSystem?_cursor=...
        // or: /fhir/CodeSystem?_cursor=...
        let path = url;

        // Remove protocol and host if present
        try {
          const urlObj = new URL(url);
          path = urlObj.pathname + urlObj.search;
        } catch {
          // If URL parsing fails, assume it's already a path
        }

        // Remove /fhir prefix if present (since we add it in fetch)
        if (path.startsWith("/fhir/")) {
          path = path.substring("/fhir/".length);
        } else if (path.startsWith("/")) {
          path = path.substring(1);
        }

        // Update endpoint state
        setEndpoint(path);

        // Update URL params
        const params = new URLSearchParams(searchParams.toString());
        params.set("endpoint", path);
        params.set("method", "GET");
        const newUrl = params.toString() ? `${pathname}?${params}` : pathname;
        navigate({ to: newUrl });

        // Trigger request
        setTimeout(() => {
          handleApiRequestRef.current();
        }, 100);
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : "Navigation failed";
        toast.error(errorMessage);
      }
    },
    [navigate, pathname, location.search]
  );

  const addHeader = () => {
    if (!headerKey.trim() || !headerValue.trim()) {
      toast.error("Both header name and value are required");
      return;
    }

    setCustomHeaders([
      ...customHeaders,
      {
        id: crypto.randomUUID(),
        key: headerKey.trim(),
        value: headerValue.trim(),
      },
    ]);
    setHeaderKey("");
    setHeaderValue("");
    setDialogOpen(false);
    toast.success("Header added");
  };

  const removeHeader = (id: string) => {
    setCustomHeaders(customHeaders.filter((h) => h.id !== id));
  };

  if (metadataQuery.isError) {
    return <ErrorArea error={metadataQuery.error} />;
  }

  return (
    <div className="flex flex-col h-full">
      <div className="shrink-0 px-6 pt-6">
        <PageHeader title="API" description="Make FHIR requests to any endpoint" />
      </div>
      {/* Top Section: Method & Endpoint */}
      <div className="p-4 border-b bg-background shrink-0">
        <div className="flex gap-3 mb-3">
          <Select
            value={httpMethod}
            onValueChange={(v) => handleMethodChange(v as HttpMethod)}
          >
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="GET">GET</SelectItem>
              <SelectItem value="POST">POST</SelectItem>
              <SelectItem value="PUT">PUT</SelectItem>
              <SelectItem value="PATCH">PATCH</SelectItem>
              <SelectItem value="DELETE">DELETE</SelectItem>
            </SelectContent>
          </Select>

          <div className="flex-1">
            <FhirInput
              searchQuery={endpoint}
              setSearchQuery={setEndpoint}
              loading={loading}
              handleSearch={handleSearch}
              resourceType={resourceType}
              capabilityStatement={metadataQuery.data}
              actionButtons={
                <>
                  <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
                    <DialogTrigger asChild>
                      <Button variant="outline" size="sm" className="h-10">
                        <Settings2 className="h-4 w-4 mr-2" />
                        Headers
                      </Button>
                    </DialogTrigger>
                    <DialogContent className="sm:max-w-md">
                      <DialogHeader>
                        <DialogTitle>Add Custom Header</DialogTitle>
                        <DialogDescription>
                          Add a custom HTTP header to the request.
                        </DialogDescription>
                      </DialogHeader>
                      <div className="space-y-4 py-4">
                        <div className="space-y-2">
                          <Label htmlFor="header-key">Header Name</Label>
                          <Input
                            id="header-key"
                            value={headerKey}
                            onChange={(e) => setHeaderKey(e.target.value)}
                            placeholder="e.g., X-Custom-Header"
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                addHeader();
                              }
                            }}
                          />
                        </div>
                        <div className="space-y-2">
                          <Label htmlFor="header-value">Header Value</Label>
                          <Input
                            id="header-value"
                            value={headerValue}
                            onChange={(e) => setHeaderValue(e.target.value)}
                            placeholder="e.g., custom-value"
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                addHeader();
                              }
                            }}
                          />
                        </div>
                      </div>
                      <DialogFooter>
                        <Button
                          type="button"
                          variant="outline"
                          onClick={() => {
                            setDialogOpen(false);
                            setHeaderKey("");
                            setHeaderValue("");
                          }}
                        >
                          Cancel
                        </Button>
                        <Button type="button" onClick={addHeader}>
                          Add Header
                        </Button>
                      </DialogFooter>
                    </DialogContent>
                  </Dialog>
                </>
              }
            />
          </div>
        </div>
      </div>

      {/* Main Content: Body and Response stacked */}
      <div className="flex-1 overflow-auto p-4 space-y-4 flex flex-col min-h-0">
        {/* Custom Headers */}
        {customHeaders.length > 0 && (
          <Card className="shrink-0">
            <CardHeader className="py-3">
              <div className="flex items-center justify-between">
                <CardTitle className="text-base">Custom Headers</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="pb-4 px-4 pt-0">
              <div className="flex flex-wrap gap-2">
                {customHeaders.map((header) => (
                  <Badge
                    key={header.id}
                    variant="secondary"
                    className="pl-3 pr-1 py-1.5 text-xs font-mono"
                  >
                    <span className="font-semibold">{header.key}:</span>
                    <span className="ml-1 text-muted-foreground">
                      {header.value}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => removeHeader(header.id)}
                      className="h-5 w-5 ml-2 hover:bg-destructive/20"
                    >
                      <X className="h-3 w-3" />
                    </Button>
                  </Badge>
                ))}
              </div>
            </CardContent>
          </Card>
        )}

        {!response && !requiresBody && !loading && (
          <div className="flex-1 flex items-center justify-center">
            <div className="text-center space-y-3">
              <Empty>
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Send className="h-8 w-8 text-muted-foreground" />
                  </EmptyMedia>
                  <EmptyTitle>Ready to Make a Request</EmptyTitle>
                  <EmptyDescription>
                    Enter a FHIR endpoint above and click Send to make a
                    request. You can also use Cmd/Ctrl + Enter as a shortcut.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            </div>
          </div>
        )}

        {/* Request Body */}
        {requiresBody && (
          <Card
            className={cn(
              "flex flex-col flex-1 p-0 overflow-hidden gap-0",
              response && "min-h-[50vh] shrink-0"
            )}
          >
            <div className="border-b py-4 px-4 shrink-0">
              <div className="flex items-center justify-between">
                <CardTitle className="text-base">Request Body</CardTitle>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7"
                  onClick={handleFormatJson}
                  disabled={!requestBody.trim()}
                >
                  <Code className="h-3 w-3 mr-1" />
                  Format
                </Button>
              </div>
            </div>
            <CardContent className="p-0 flex-1 min-h-0 overflow-hidden">
              <textarea
                value={requestBody}
                onChange={(e) => setRequestBody(e.target.value)}
                placeholder='{\n  "resourceType": "Patient",\n  "name": [{\n    "family": "Doe",\n    "given": ["John"]\n  }]\n}'
                className="w-full h-full px-4 py-3 bg-card font-mono text-xs focus:outline-none resize-none"
              />
            </CardContent>
          </Card>
        )}

        {response && (
          <Card className={cn("gap-0 p-0 overflow-hidden")}>
            <div className="border-b py-4 px-4 shrink-0">
              <div className="flex items-center justify-between">
                <CardTitle className="text-base">Response</CardTitle>
                <div className="flex items-center gap-2">
                  {Object.keys(response.headers).length > 0 && (
                    <Dialog
                      open={responseHeadersDialogOpen}
                      onOpenChange={setResponseHeadersDialogOpen}
                    >
                      <DialogTrigger asChild>
                        <Button variant="outline" size="sm" className="h-7">
                          Headers
                        </Button>
                      </DialogTrigger>
                      <DialogContent className="sm:max-w-2xl">
                        <DialogHeader>
                          <DialogTitle>Response Headers</DialogTitle>
                          <DialogDescription>
                            HTTP headers returned by the server.
                          </DialogDescription>
                        </DialogHeader>
                        <div className="max-h-96 overflow-y-auto">
                          <div className="border rounded-md bg-muted/30">
                            {Object.entries(response.headers).map(
                              ([key, value]) => (
                                <div
                                  key={key}
                                  className="px-4 py-2.5 text-sm font-mono border-b last:border-b-0 flex gap-3"
                                >
                                  <span className="font-semibold text-muted-foreground min-w-[180px] shrink-0">
                                    {key}:
                                  </span>
                                  <span className="flex-1 break-all">
                                    {value}
                                  </span>
                                </div>
                              )
                            )}
                          </div>
                        </div>
                      </DialogContent>
                    </Dialog>
                  )}
                  <StatusBadge status={response.status} />
                  <span className="text-sm text-muted-foreground">
                    {response.statusText}
                  </span>
                </div>
              </div>
            </div>
            <CardContent
              className={cn(
                "p-4 pt-0",
                requiresBody ? "flex-1 min-h-0 overflow-auto" : ""
              )}
            >
              {response.error ? (
                <div className="p-4 border border-destructive rounded-md bg-destructive/10 text-destructive">
                  {response.error}
                </div>
              ) : (
                <ResultTabs
                  data={response.data as FhirResponse}
                  onNavigate={handleBundleNavigation}
                />
              )}
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  );
}
