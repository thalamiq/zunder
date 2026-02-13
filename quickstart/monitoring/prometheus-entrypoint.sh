#!/bin/sh
set -e

# Substitute FHIR_SERVER_TARGET in the prometheus config template
# Using sed instead of envsubst for better compatibility
sed "s|\${FHIR_SERVER_TARGET:-fhir-server:8080}|${FHIR_SERVER_TARGET:-fhir-server:8080}|g" \
    /etc/prometheus/prometheus.yml.template > /etc/prometheus/prometheus.yml

# Start Prometheus with the provided arguments
exec /bin/prometheus "$@"
