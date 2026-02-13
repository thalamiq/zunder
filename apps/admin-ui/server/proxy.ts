import { NextRequest, NextResponse } from "next/server";
import { FHIR_SERVER_URL } from "../lib/config";

export interface ProxyConfig {
  /** The target path prefix (e.g., '/fhir' or '/admin') */
  targetPathPrefix: string;
  /** Default Accept header if not provided in request */
  defaultAccept: string;
  /** Error message for proxy failures */
  errorMessage: string;
  /** Whether to forward Location header in responses */
  forwardLocation?: boolean;
}

/**
 * Shared proxy function for forwarding requests to the FHIR server
 */
export async function proxyRequest(
  request: NextRequest,
  params: Promise<{ path?: string[] }>,
  method: string,
  config: ProxyConfig
): Promise<NextResponse> {
  // Resolve params early so we can use it in error handling
  const resolvedParams = await params;
  const path = resolvedParams?.path;
  const pathSegment = (path ?? []).join("/");

  // Build the target URL (needed for both success and error cases)
  const targetPath = pathSegment
    ? `${config.targetPathPrefix}/${pathSegment}`
    : config.targetPathPrefix;
  const searchParams = request.nextUrl.searchParams.toString();
  const targetUrl = `${FHIR_SERVER_URL}${targetPath}${
    searchParams ? `?${searchParams}` : ""
  }`;

  try {
    // Prepare headers
    const headers = new Headers();

    // Forward relevant headers from the original request
    const contentType = request.headers.get("content-type");
    if (contentType) {
      headers.set("content-type", contentType);
    }

    const accept = request.headers.get("accept");
    if (accept) {
      headers.set("accept", accept);
    } else {
      headers.set("accept", config.defaultAccept);
    }

    // Forward authorization if present
    const authorization = request.headers.get("authorization");
    if (authorization) {
      headers.set("authorization", authorization);
    }

    // Forward cookies (required for admin UI session cookie auth).
    const cookie = request.headers.get("cookie");
    if (cookie) {
      headers.set("cookie", cookie);
    }

    // Forward proxy headers if present (helps the backend set Secure cookies correctly).
    const xForwardedProto = request.headers.get("x-forwarded-proto");
    if (xForwardedProto) {
      headers.set("x-forwarded-proto", xForwardedProto);
    }
    const xForwardedHost = request.headers.get("x-forwarded-host");
    if (xForwardedHost) {
      headers.set("x-forwarded-host", xForwardedHost);
    }

    // Prepare request options
    const requestOptions: RequestInit = {
      method,
      headers,
    };

    // Add body for methods that support it
    if (["POST", "PUT", "PATCH"].includes(method)) {
      const body = await request.text();
      if (body) {
        requestOptions.body = body;
      }
    }

    // Make the proxied request
    console.log(`Proxying ${method} request to: ${targetUrl}`);
    const response = await fetch(targetUrl, requestOptions);

    // Get response body (204 No Content must have null body)
    const responseText = response.status === 204 ? null : await response.text();

    // Create response with same status and headers
    const proxiedResponse = new NextResponse(responseText, {
      status: response.status,
      statusText: response.statusText,
    });

    // Forward relevant response headers
    const contentTypeResponse = response.headers.get("content-type");
    if (contentTypeResponse) {
      proxiedResponse.headers.set("content-type", contentTypeResponse);
    }

    const setCookie = response.headers.get("set-cookie");
    if (setCookie) {
      proxiedResponse.headers.append("set-cookie", setCookie);
    }

    const wwwAuthenticate = response.headers.get("www-authenticate");
    if (wwwAuthenticate) {
      proxiedResponse.headers.set("www-authenticate", wwwAuthenticate);
    }

    if (config.forwardLocation) {
      const location = response.headers.get("location");
      if (location) {
        proxiedResponse.headers.set("location", location);
      }
    }

    return proxiedResponse;
  } catch (error) {
    console.error("Proxy error:", {
      error: error instanceof Error ? error.message : String(error),
      targetUrl,
      method,
      cause:
        error instanceof Error && "cause" in error ? error.cause : undefined,
    });

    const errorMessage =
      error instanceof Error &&
      "cause" in error &&
      typeof error.cause === "object" &&
      error.cause !== null &&
      "code" in error.cause &&
      error.cause.code === "ECONNREFUSED"
        ? `Failed to connect to FHIR server at ${targetUrl}. Is the server running?`
        : config.errorMessage;

    const details = error instanceof Error ? error.message : String(error);

    // Return OperationOutcome per FHIR spec for error responses
    return NextResponse.json(
      {
        resourceType: "OperationOutcome",
        issue: [
          {
            severity: "error",
            code: "exception",
            diagnostics: errorMessage,
            details: {
              text: details,
            },
          },
        ],
      },
      {
        status: 502,
        headers: {
          "Content-Type": "application/fhir+json",
        },
      }
    );
  }
}

/**
 * Creates HTTP method handlers for a proxy route
 */
export function createProxyHandlers(config: ProxyConfig) {
  const handlers = {
    GET: async (
      request: NextRequest,
      context?: { params?: Promise<{ path?: string[] }> }
    ) =>
      proxyRequest(
        request,
        context?.params ?? Promise.resolve({}),
        "GET",
        config
      ),

    POST: async (
      request: NextRequest,
      context?: { params?: Promise<{ path?: string[] }> }
    ) =>
      proxyRequest(
        request,
        context?.params ?? Promise.resolve({}),
        "POST",
        config
      ),

    PUT: async (
      request: NextRequest,
      context?: { params?: Promise<{ path?: string[] }> }
    ) =>
      proxyRequest(
        request,
        context?.params ?? Promise.resolve({}),
        "PUT",
        config
      ),

    PATCH: async (
      request: NextRequest,
      context?: { params?: Promise<{ path?: string[] }> }
    ) =>
      proxyRequest(
        request,
        context?.params ?? Promise.resolve({}),
        "PATCH",
        config
      ),

    DELETE: async (
      request: NextRequest,
      context?: { params?: Promise<{ path?: string[] }> }
    ) =>
      proxyRequest(
        request,
        context?.params ?? Promise.resolve({}),
        "DELETE",
        config
      ),
  };

  return handlers;
}
