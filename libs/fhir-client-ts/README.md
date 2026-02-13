# @thalamiq/fhir-client

A modern TypeScript FHIR client for Zunder.

## Installation

```bash
npm install @thalamiq/fhir-client
```

## Basic Setup

```typescript
import { FhirClient } from "@thalamiq/fhir-client";

const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  timeout: 30000, // optional, default: 30000ms
  headers: {
    // optional custom headers
  },
});
```

## Authentication

### Bearer Token

```typescript
const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  auth: {
    token: "your-access-token",
  },
});
```

### Token Provider (for dynamic tokens)

```typescript
const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  auth: {
    tokenProvider: async () => {
      // Fetch or refresh token
      return "your-access-token";
    },
  },
});
```

### Client Credentials (Machine-to-Machine)

```typescript
import { FhirClient, ClientCredentialsAuth } from "@thalamiq/fhir-client";

const auth = new ClientCredentialsAuth({
  clientId: "your-client-id",
  clientSecret: "your-client-secret",
  issuer: "https://auth.example.com", // or tokenEndpoint
  scope: "system/*.*",
});

const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  auth: {
    tokenProvider: auth.tokenProvider(),
  },
});
```

### SMART on FHIR (Authorization Code Flow)

SMART on FHIR provides secure user authentication with automatic token refresh.

#### Browser Example

```typescript
import { FhirClient, SmartAuth } from "@thalamiq/fhir-client";

// Initialize SMART auth
const smartAuth = new SmartAuth({
  fhirBaseUrl: "https://fhir.example.com",
  clientId: "your-client-id",
  redirectUri: window.location.origin + "/callback",
  scope: "openid profile fhirUser user/*.*",
});

// Check if handling callback
const urlParams = new URLSearchParams(window.location.search);
const code = urlParams.get("code");
const state = urlParams.get("state");
const error = urlParams.get("error");

if (code && state) {
  // Handle callback
  try {
    await smartAuth.handleCallback(code, state);
    // Redirect to app
    window.location.href = "/app";
  } catch (err) {
    console.error("Authentication failed:", err);
  }
} else if (!smartAuth.isAuthenticated()) {
  // Initiate login
  await smartAuth.authorize(); // Redirects to auth server
}

// Create client with SMART auth
const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  auth: {
    tokenProvider: smartAuth.tokenProvider(),
  },
});

// Get current patient context (if available)
const patientId = smartAuth.getPatientId();
if (patientId) {
  console.log("Current patient:", patientId);
}

// Get user scopes
const scopes = smartAuth.getScopes();
console.log("User scopes:", scopes);
```

#### React Example

```typescript
import { useEffect, useState } from "react";
import { FhirClient, SmartAuth } from "@thalamiq/fhir-client";

function App() {
  const [client, setClient] = useState<FhirClient | null>(null);
  const [smartAuth, setSmartAuth] = useState<SmartAuth | null>(null);

  useEffect(() => {
    const auth = new SmartAuth({
      fhirBaseUrl: "https://fhir.example.com",
      clientId: "your-client-id",
      redirectUri: window.location.origin + "/callback",
    });

    setSmartAuth(auth);

    // Handle callback
    const urlParams = new URLSearchParams(window.location.search);
    const code = urlParams.get("code");
    const state = urlParams.get("state");

    if (code && state) {
      auth.handleCallback(code, state).then(() => {
        window.history.replaceState({}, "", window.location.pathname);
        initializeClient(auth);
      });
    } else if (auth.isAuthenticated()) {
      initializeClient(auth);
    } else {
      auth.authorize(); // Redirects to login
    }
  }, []);

  const initializeClient = (auth: SmartAuth) => {
    const fhirClient = new FhirClient({
      baseUrl: "https://fhir.example.com",
      auth: {
        tokenProvider: auth.tokenProvider(),
      },
    });
    setClient(fhirClient);
  };

  const handleLogout = () => {
    smartAuth?.logout();
    window.location.reload();
  };

  if (!client) {
    return <div>Authenticating...</div>;
  }

  return (
    <div>
      <button onClick={handleLogout}>Logout</button>
      {/* Your app content */}
    </div>
  );
}
```

#### Custom Storage (for Node.js or custom storage)

```typescript
import { SmartAuth, type SmartAuthStorage } from "@thalamiq/fhir-client";

// Custom storage implementation
const customStorage: SmartAuthStorage = {
  get: (key: string) => {
    // Your storage logic
    return localStorage.getItem(key);
  },
  set: (key: string, value: string) => {
    localStorage.setItem(key, value);
  },
  remove: (key: string) => {
    localStorage.removeItem(key);
  },
};

const smartAuth = new SmartAuth({
  fhirBaseUrl: "https://fhir.example.com",
  clientId: "your-client-id",
  redirectUri: "https://your-app.com/callback",
  storage: customStorage, // Use custom storage
});
```

## CRUD Operations

### Create

```typescript
import type { Patient } from "fhir/r4";

const patient: Patient = {
  resourceType: "Patient",
  name: [
    {
      family: "Doe",
      given: ["John"],
    },
  ],
};

const result = await client.create(patient);
console.log(result.resource.id); // Created resource ID
console.log(result.meta.location); // Resource location
```

### Read

```typescript
const result = await client.read<Patient>("Patient", "123");

// With summary
const summary = await client.read<Patient>("Patient", "123", {
  summary: "true",
});

// With specific elements
const elements = await client.read<Patient>("Patient", "123", {
  elements: ["name", "birthDate"],
});
```

### Version Read (vread)

```typescript
const result = await client.vread<Patient>("Patient", "123", "v1");
```

### Update

```typescript
patient.id = "123";
patient.name[0].given.push("Middle");

const result = await client.update(patient);
```

### Patch

```typescript
import type { JsonPatchOperation } from "@thalamiq/fhir-client";

const operations: JsonPatchOperation[] = [
  {
    op: "replace",
    path: "/name/0/given/0",
    value: "Jane",
  },
];

const result = await client.patch<Patient>("Patient", "123", operations);
```

### Delete

```typescript
const result = await client.delete("Patient", "123");
```

## Search Operations

Search operations use a simple object-based approach for search parameters.

### Basic Search

```typescript
// Search all patients
const result = await client.search<Patient>("Patient");

// Search with parameters
const result = await client.search<Patient>("Patient", {
  name: "Doe",
  birthdate: "2020-01-01",
});
```

### Search with Multiple Values

```typescript
// Multiple values for the same parameter (OR condition)
const result = await client.search<Patient>("Patient", {
  name: ["Doe", "Smith"],
});
```

### Search with Modifiers

FHIR search modifiers are supported by including them in the parameter name:

```typescript
// Exact match
const result = await client.search<Patient>("Patient", {
  "name:exact": "John Doe",
});

// Contains
const result = await client.search<Patient>("Patient", {
  "name:contains": "John",
});

// Missing
const result = await client.search<Patient>("Patient", {
  "birthdate:missing": "true",
});

// Not
const result = await client.search<Patient>("Patient", {
  "name:not": "Doe",
});
```

### Search with System Parameters

```typescript
const result = await client.search<Patient>("Patient", {
  name: "Doe",
  _count: "10",
  _sort: "name",
  _summary: "true",
  _elements: "name,birthDate",
  _include: "Patient:organization",
  _revinclude: "Encounter:patient",
});
```

### Pagination

```typescript
const result = await client.search<Patient>("Patient", {
  _count: "10",
});

console.log(result.total); // Total number of results
console.log(result.resources); // Array of Patient resources
console.log(result.hasNextPage()); // Check if next page exists
console.log(result.hasPrevPage()); // Check if previous page exists

// Navigate to next page
if (result.hasNextPage()) {
  const nextPage = await result.nextPage();
  console.log(nextPage.resources);
}

// Navigate to previous page
if (result.hasPrevPage()) {
  const prevPage = await result.prevPage();
  console.log(prevPage.resources);
}
```

### Search Options

```typescript
const abortController = new AbortController();

const result = await client.search<Patient>(
  "Patient",
  { name: "Doe" },
  {
    headers: {
      "X-Custom-Header": "value",
    },
    signal: abortController.signal,
  }
);

// Cancel request
abortController.abort();
```

## Conditional Operations

### Conditional Create

```typescript
const result = await client.conditionalCreate(patient, "identifier=12345");
```

### Conditional Update

```typescript
const result = await client.conditionalUpdate(patient, "identifier=12345");
```

### Conditional Delete

```typescript
const result = await client.conditionalDelete("Patient", "identifier=12345");
```

## Batch and Transaction Operations

### Batch

```typescript
import { BundleBuilder } from "@thalamiq/fhir-client";

const bundle = client
  .batch()
  .create(patient1)
  .create(patient2)
  .read("Patient", "123")
  .execute();

const result = await bundle;
console.log(result.bundle.entry);
```

### Transaction

```typescript
const bundle = client
  .transaction()
  .create(patient1)
  .update(patient2)
  .delete("Patient", "123")
  .execute();

const result = await bundle;
```

## History Operations

### Resource History

```typescript
const result = await client.history("Patient", "123", {
  count: 10,
  since: new Date("2024-01-01"),
});
```

### Type History

```typescript
const result = await client.typeHistory("Patient", {
  count: 20,
});
```

### System History

```typescript
const result = await client.systemHistory({
  count: 50,
  since: "2024-01-01T00:00:00Z",
});
```

## Operations

### System-Level Operation

```typescript
import type { Parameters } from "fhir/r4";

const params: Parameters = {
  resourceType: "Parameters",
  parameter: [
    {
      name: "name",
      valueString: "value",
    },
  ],
};

const result = await client.operation("validate", params);
```

### Type-Level Operation

```typescript
const result = await client.typeOperation("Patient", "everything", params);
```

### Instance-Level Operation

```typescript
const result = await client.instanceOperation("Patient", "123", "everything", params);
```

### GET Operation

```typescript
const result = await client.operation("validate", params, {
  method: "GET",
});
```

## Capabilities

```typescript
const capabilities = await client.capabilities();
console.log(capabilities.capabilityStatement);
```

## Error Handling

```typescript
import {
  FhirError,
  NotFoundError,
  ValidationError,
  NetworkError,
  TimeoutError,
} from "@thalamiq/fhir-client";

try {
  const result = await client.read<Patient>("Patient", "123");
} catch (error) {
  if (error instanceof NotFoundError) {
    console.log("Patient not found");
  } else if (error instanceof ValidationError) {
    console.log("Validation error:", error.operationOutcome);
  } else if (error instanceof NetworkError) {
    console.log("Network error:", error.message);
  } else if (error instanceof TimeoutError) {
    console.log("Request timed out");
  } else if (error instanceof FhirError) {
    console.log("FHIR error:", error.status, error.operationOutcome);
  }
}
```

## TypeScript Support

The client is fully typed with FHIR R4 types from `@types/fhir`:

```typescript
import type { Patient, Observation, Bundle } from "fhir/r4";

const patient: Patient = {
  resourceType: "Patient",
  // ... TypeScript will provide autocomplete and type checking
};

const result = await client.read<Patient>("Patient", "123");
// result.resource is typed as Patient
```

## Examples

### Complete Example

```typescript
import { FhirClient } from "@thalamiq/fhir-client";
import type { Patient, Observation } from "fhir/r4";

const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  auth: {
    token: "your-token",
  },
});

// Create a patient
const patient: Patient = {
  resourceType: "Patient",
  name: [{ family: "Doe", given: ["John"] }],
  birthDate: "1990-01-01",
};

const created = await client.create(patient);
console.log("Created patient:", created.resource.id);

// Search for patients
const searchResult = await client.search<Patient>("Patient", {
  name: "Doe",
  _count: "10",
});

console.log(`Found ${searchResult.total} patients`);

// Paginate through results
for (const patient of searchResult.resources) {
  console.log(patient.name);
}

if (searchResult.hasNextPage()) {
  const nextPage = await searchResult.nextPage();
  console.log("Next page:", nextPage.resources);
}

// Read a specific patient
const readResult = await client.read<Patient>("Patient", created.resource.id!);
console.log("Read patient:", readResult.resource);

// Search for observations for this patient
const observations = await client.search<Observation>("Observation", {
  patient: `Patient/${created.resource.id}`,
  _sort: "-date",
  _count: "5",
});

console.log("Observations:", observations.resources);
```

### SMART on FHIR Example

```typescript
import { FhirClient, SmartAuth } from "@thalamiq/fhir-client";
import type { Patient, Observation } from "fhir/r4";

// Initialize SMART auth
const smartAuth = new SmartAuth({
  fhirBaseUrl: "https://fhir.example.com",
  clientId: "your-client-id",
  redirectUri: window.location.origin + "/callback",
  scope: "openid profile fhirUser user/Patient.read user/Observation.read",
});

// Handle callback if present
const urlParams = new URLSearchParams(window.location.search);
if (urlParams.get("code")) {
  await smartAuth.handleCallback(
    urlParams.get("code")!,
    urlParams.get("state")!
  );
}

// Create authenticated client
const client = new FhirClient({
  baseUrl: "https://fhir.example.com",
  auth: {
    tokenProvider: smartAuth.tokenProvider(),
  },
});

// Get current patient context
const patientId = smartAuth.getPatientId();
if (patientId) {
  // Read current patient
  const patient = await client.read<Patient>("Patient", patientId);
  console.log("Current patient:", patient.resource.name);

  // Search observations for current patient
  const observations = await client.search<Observation>("Observation", {
    patient: `Patient/${patientId}`,
    _sort: "-date",
    _count: "10",
  });

  console.log(`Found ${observations.total} observations`);
  observations.resources.forEach((obs) => {
    console.log(obs.code, obs.valueQuantity);
  });
}
```

## License

MIT
