#!/usr/bin/env sh
set -e

# Zunder FHIR Server Installer
# This script downloads and starts the FHIR server stack

VERSION="${ZUNDER_VERSION:-v0.1.0}"
REPO="${ZUNDER_REPO:-thalamiq/zunder}"
INSTALL_DIR="${ZUNDER_HOME:-./zunder}"

echo "Installing Zunder FHIR Server ${VERSION}"
echo "Install directory: ${INSTALL_DIR}"

BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"

# Download and extract distribution
echo "Downloading distribution package..."
mkdir -p "${INSTALL_DIR}"
cd "${INSTALL_DIR}"

# Download tarball and extract
if ! curl -fsSL "${BASE_URL}/zunder.tar.gz" | tar xz; then
  echo "Error: Failed to download or extract distribution from ${BASE_URL}/zunder.tar.gz"
  echo "Please check that the release exists and try again."
  exit 1
fi

# Check if extraction created a subdirectory
if [ -d "quickstart" ]; then
  # Move contents up one level
  mv quickstart/* quickstart/.* . 2>/dev/null || true
  rmdir quickstart
fi

echo "Distribution extracted successfully"

echo ""
echo "Configuration:"
echo "  - Edit config.yaml to customize FHIR server settings"
echo "  - Database credentials: Set POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB if needed"
echo "  - Default credentials: fhir/fhir"
echo ""

# Ensure .env exists so compose doesn't fail (env_file)
if [ ! -f .env ]; then
  cp .env.example .env 2>/dev/null || touch .env
fi

echo "Starting services..."
docker compose up -d

echo ""
echo "✓ FHIR server is starting..."
echo ""
echo "Quick commands:"
echo "  Status:     docker compose ps"
echo "  Logs:       docker compose logs -f"
echo "  Stop:       docker compose down"
echo ""
echo "Optional: Enable monitoring stack"
echo "  docker compose -f compose.yaml -f compose.monitoring.yaml up -d"
echo "  Then visit http://localhost:3000 (admin/admin)"
echo ""
