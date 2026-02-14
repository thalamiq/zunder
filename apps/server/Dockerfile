# syntax=docker/dockerfile:1.7
#
# Multi-stage build for FHIR Server (cargo-chef for dependency caching)
# Build context: repository root (not apps/server/)

ARG RUST_VERSION=1.92
ARG BUILD_DATE
ARG VCS_REF
ARG VERSION=0.1.0

############################
# Chef - base with cargo-chef
############################
FROM rust:${RUST_VERSION}-slim-bookworm AS chef

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install cargo-chef --locked

WORKDIR /build

############################
# Planner - generate dependency recipe
############################
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY apps/server/Cargo.toml apps/server/Cargo.toml
COPY apps/cli/Cargo.toml apps/cli/Cargo.toml
COPY libs/fhir-models/Cargo.toml libs/fhir-models/Cargo.toml
COPY libs/fhirpath/Cargo.toml libs/fhirpath/Cargo.toml
COPY libs/fhir-context/Cargo.toml libs/fhir-context/Cargo.toml
COPY libs/fhir-format/Cargo.toml libs/fhir-format/Cargo.toml
COPY libs/fhir-snapshot/Cargo.toml libs/fhir-snapshot/Cargo.toml
COPY libs/fhir-package/Cargo.toml libs/fhir-package/Cargo.toml
COPY libs/fhir-registry-client/Cargo.toml libs/fhir-registry-client/Cargo.toml
COPY libs/fhir-codegen/Cargo.toml libs/fhir-codegen/Cargo.toml
COPY libs/fhir-validator/Cargo.toml libs/fhir-validator/Cargo.toml
COPY libs/ucum/Cargo.toml libs/ucum/Cargo.toml

# Stub sources so cargo can resolve the workspace
RUN mkdir -p apps/server/src apps/cli/src && \
    echo "fn main() {}" > apps/server/src/main.rs && \
    echo "fn main() {}" > apps/cli/src/main.rs && \
    for lib in libs/fhir-models libs/fhirpath libs/fhir-context libs/fhir-format \
               libs/fhir-snapshot libs/fhir-package libs/fhir-registry-client \
               libs/fhir-codegen libs/fhir-validator libs/ucum; do \
        mkdir -p "$lib/src" && echo "" > "$lib/src/lib.rs"; \
    done

RUN cargo chef prepare --recipe-path recipe.json

############################
# Builder
############################
FROM chef AS builder

# Cook dependencies (cached layer — only rebuilds when Cargo.toml/lock change)
COPY --from=planner /build/recipe.json recipe.json
# Create bench stubs that cargo-chef doesn't generate
RUN mkdir -p libs/fhirpath/benches && echo "fn main() {}" > libs/fhirpath/benches/benchmark.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --recipe-path recipe.json

# Copy real source
COPY Cargo.toml Cargo.lock ./
COPY libs/ libs/
COPY apps/server/ apps/server/
# CLI manifest needed for workspace resolution (source not needed)
COPY apps/cli/Cargo.toml apps/cli/Cargo.toml
RUN mkdir -p apps/cli/src && echo "fn main() {}" > apps/cli/src/main.rs

# Build application (deps already compiled by cook)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked --bin fhir-server --bin fhir-worker

RUN strip target/release/fhir-server target/release/fhir-worker

############################
# Runtime
############################
FROM debian:bookworm-slim

ARG BUILD_DATE
ARG VCS_REF
ARG VERSION

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    tini \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1001 -s /usr/sbin/nologin fhir && \
    mkdir -p /app/migrations /app/data && \
    chown -R fhir:fhir /app && \
    mkdir -p /home/fhir/.fhir/packages && \
    chown -R fhir:fhir /home/fhir/.fhir

WORKDIR /app

COPY --from=builder /build/target/release/fhir-server /usr/local/bin/fhir-server
COPY --from=builder /build/target/release/fhir-worker /usr/local/bin/fhir-worker
COPY --chown=fhir:fhir apps/server/migrations ./migrations
COPY --chown=fhir:fhir apps/server/fhir_packages ./fhir_packages
COPY --chown=root:root apps/server/scripts/start-all.sh /usr/local/bin/start-all
RUN chmod 0755 /usr/local/bin/start-all

RUN chown root:root /usr/local/bin/fhir-server /usr/local/bin/fhir-worker && \
    chmod 0755 /usr/local/bin/fhir-server /usr/local/bin/fhir-worker

LABEL org.opencontainers.image.title="Ferrum FHIR Server" \
      org.opencontainers.image.description="A spec compliant and performant FHIR server" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.vendor="ThalamiQ" \
      maintainer="ThalamiQ"

USER fhir

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -fsS http://localhost:8080/health || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["fhir-server"]
