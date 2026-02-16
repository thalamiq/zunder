# Ferrum Admin UI

Admin dashboard for the Ferrum FHIR Server. Built with Vite, React, TanStack Router, and TanStack Query.

## Development

Start the FHIR server (port 8080), then run the UI dev server:

```bash
pnpm dev
```

Open http://localhost:5173/ui/ — the Vite dev server proxies `/admin`, `/fhir`, and `/health` requests to `http://127.0.0.1:8080`.

## Production

In production the UI is built as static files and served by the Rust server at `/ui/`. No separate container is needed.

```bash
pnpm build   # outputs to dist/
```

The server Dockerfile includes a UI build stage that copies `dist/` into the image automatically.
