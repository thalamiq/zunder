#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
DIST_DIR="${REPO_ROOT}/quickstart"
TARBALL="${1:-zunder.tar.gz}"

if [ ! -d "$DIST_DIR" ]; then
  echo "quickstart/ directory not found at ${DIST_DIR}"
  exit 1
fi

tar -czf "$TARBALL" -C "$DIST_DIR" .
echo "Created ${TARBALL}"
