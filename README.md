<div align="center">

# Ferrum

**A fast FHIR R4/R5 server built in Rust.**

[Documentation](https://docs.ferrum.thalamiq.io) | [Live Demo](https://ferrum.thalamiq.io) | [Test API](https://api.ferrum.thalamiq.io/fhir/metadata)

</div>

---

Ferrum is a complete [FHIR](https://hl7.org/fhir/) ecosystem for healthcare interoperability: CRUD, search, batch/transaction bundles, terminology services, validation, and more. Developed by [ThalamiQ](https://thalamiq.io).

## Quickstart

```bash
curl -fsSL https://get.ferrum.thalamiq.io | sh
```

This starts the FHIR server, database, and admin UI. Access the API at `localhost:8080/fhir` and the admin UI at `localhost:3000`.

> This project is under active development. APIs may change at any time. If you encounter issues or spec compliance gaps, please [open an issue](https://github.com/thalamiq/ferrum/issues).

## Features

| | |
|---|---|
| **FHIR R4/R5 REST API** | CRUD, conditional operations, search, batch/transaction bundles |
| **Advanced Search** | Chaining, `_include`/`_revinclude`, full-text search, compartments |
| **Terminology Services** | `$expand`, `$lookup`, `$validate-code`, `$subsumes`, `$translate`, `$closure` |
| **FHIRPath Engine** | Full expression evaluator for querying and transforming resources |
| **Validation** | Resource validation against profiles and constraints |
| **Snapshot Generation** | StructureDefinition snapshots from differentials |
| **SMART on FHIR** | OIDC based authentication and authorization |
| **Admin UI** | Web dashboard for resource browsing, monitoring, and administration |

## Architecture

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│   Admin UI   │    │  FHIR Server │    │    Worker    │
│   (Next.js)  │───▶│    (Axum)    │◀──▶│  (Background) │
└──────────────┘    └──────┬───────┘    └──────┬───────┘
                           │                   │
                           ▼                   ▼
                    ┌──────────────────────────────┐
                    │          PostgreSQL           │
                    └──────────────────────────────┘
```

## Documentation

Full documentation at [docs.ferrum.thalamiq.io](https://docs.ferrum.thalamiq.io).

## License

Licensed under the Apache License, Version 2.0. Copyright © 2026 Thalamiq GmbH.
