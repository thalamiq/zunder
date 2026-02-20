export interface FhirPathRequest {
  expression: string;
  resource: object;
}

export interface FhirPathResponse {
  result: unknown[];
  count: number;
  elapsed_ms: number;
}

export const evaluateFhirPath = async (
  req: FhirPathRequest,
): Promise<FhirPathResponse> => {
  const response = await fetch("/admin/fhirpath/evaluate", {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });

  if (!response.ok) {
    // The server returns an OperationOutcome with diagnostics on error
    let message = response.statusText;
    try {
      const body = await response.json();
      const diagnostics = body?.issue?.[0]?.diagnostics;
      if (typeof diagnostics === "string") {
        message = diagnostics;
      }
    } catch {
      // fall back to statusText
    }
    throw new Error(message);
  }

  return response.json() as Promise<FhirPathResponse>;
};
