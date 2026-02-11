![Zunder](assets/zunder.svg)

A spec compliant and performant FHIR server implementation.

Developed by [ThalamiQ](https://thalamiq.io).

> **Note:** This project is under active development. APIs may change at any time. If you encounter issues, please [open an issue](https://github.com/thalamiq/zunder/issues).

## Quickstart

```bash
curl -fsSL https://get.yourfhir.dev | sh
```

This starts the FHIR server, database, background worker, and admin UI. Access the API at [http://localhost:8080/fhir](http://localhost:8080/fhir) and the Admin UI at [http://localhost:3000](http://localhost:3000).

## Features

- FHIR R4/R5 REST API (CRUD, Search, Batch/Transaction)
- Advanced search with chaining, includes, and full-text search
- Background job processing for indexing and terminology
- SMART on FHIR / OIDC authentication
- Admin UI for resource browsing and monitoring

## Configuration

Configuration is loaded from environment variables (prefixed with `FHIR__`), `config.yaml`, or defaults:

```bash
FHIR__DATABASE__URL=postgresql://fhir:fhir@localhost:5432/fhir
FHIR__SERVER__PORT=8080
FHIR__AUTH__ENABLED=true
```

## Documentation

Full documentation is available at [docs.thalamiq.io](https://docs.thalamiq.io/).

## License

Licensed under the Apache License, Version 2.0.
Copyright © 2026 Thalamiq GmbH.
