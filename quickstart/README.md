# Zunder - Distribution Package

Zunder is a spec compliant and performant FHIR server implementation built in Rust.

## Quick Start

### 1. Start the server

```bash
docker compose up -d
```

This starts:

- PostgreSQL database
- FHIR server (API)
- FHIR worker (background jobs)
- Admin UI

### 2. Access the services

- **FHIR API**: http://localhost:8080/fhir/metadata
- **Admin UI**: http://localhost:3000

### 3. Test it works

```bash
# Create a patient
curl -X POST http://localhost:8080/fhir/Patient \
  -H "Content-Type: application/fhir+json" \
  -d '{
    "resourceType": "Patient",
    "name": [{"family": "Smith", "given": ["John"]}]
  }'

# Search patients
curl http://localhost:8080/fhir/Patient
```

## Configuration

### FHIR Server Configuration

Edit `config.yaml` to customize FHIR server behavior:

- Search parameters
- Database connection pooling
- FHIR package installation
- Logging and audit settings
- OpenTelemetry configuration
- And much more...

See `config.yaml` for detailed documentation of all options.

### Environment Variables (Optional)

Override settings using environment variables:

**Database credentials:**

```bash
export POSTGRES_USER=fhir
export POSTGRES_PASSWORD=your_secure_password  # ⚠️ Change in production!
export POSTGRES_DB=fhir
```

**Database URL override:**

```bash
# Override the database.url from config.yaml
export DATABASE_URL="postgresql://user:pass@host/db"
# Or use the FHIR__ prefix for any config value
export FHIR__DATABASE__URL="postgresql://user:pass@host/db"
```

**Ports and binding:**

```bash
export BIND_ADDRESS=127.0.0.1    # localhost only (default)
# export BIND_ADDRESS=0.0.0.0    # Expose on all interfaces
```

**Docker image versions:**

```bash
export FHIR_SERVER_IMAGE=ghcr.io/thalamiq/zunder:v0.1.0
export FHIR_UI_IMAGE=ghcr.io/thalamiq/zunder-ui:v0.1.0
```

## Monitoring (Optional)

Includes production-ready monitoring with Prometheus + Grafana + pre-configured dashboards.

### Enable monitoring

```bash
docker compose -f compose.yaml -f compose.monitoring.yaml up -d
```

### Access Grafana

Visit http://localhost:3000 and login:

- Username: `admin`
- Password: `admin` (change via GRAFANA_PASSWORD env var)

### Pre-configured Dashboards

Three dashboards are automatically available:

1. **FHIR Server - Overview**

   - Request rates, latencies (p50, p95, p99)
   - Error rates by status code
   - Database connection pool status
   - Job queue backlog

2. **FHIR Server - Operations**

   - CRUD operations by resource type
   - Search performance
   - Operation latency breakdowns

3. **PostgreSQL - FHIR Database**
   - Connections and cache hit ratio
   - Transaction rates and database size
   - Query performance

### Monitoring Ports

Default ports (customize with environment variables):

- Grafana: http://localhost:3000 (set GRAFANA_PORT)
- Prometheus: http://localhost:9090 (set PROMETHEUS_PORT)
- Tempo: http://localhost:3200 (set TEMPO_PORT)
- PostgreSQL Exporter: 9187 (set POSTGRES_EXPORTER_PORT)
- OpenTelemetry: 4317 (set OTEL_GRPC_PORT)

See `monitoring/README.md` for detailed documentation.

## Management Commands

### Start services

```bash
docker compose up -d
```

### Stop services

```bash
docker compose down
```

### View logs

```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f fhir-server
docker compose logs -f fhir-worker
```

### Check status

```bash
docker compose ps
```

### Scale workers

```bash
docker compose up -d --scale fhir-worker=3
```

### Restart services

```bash
docker compose restart fhir-server
```

## Data Persistence

All data is stored in Docker volumes:

- `postgres_data` - Database (persistent)
- `prometheus_data` - Metrics history (if monitoring enabled)
- `grafana_data` - Dashboards and config (if monitoring enabled)

### Backup database

```bash
docker compose exec db pg_dump -U fhir fhir > backup.sql
```

### Restore database

```bash
docker compose exec -T db psql -U fhir fhir < backup.sql
```

### Remove all data

```bash
docker compose down -v  # ⚠️ Deletes all data!
```

## Production Deployment

### Security Checklist

- [ ] Change `POSTGRES_PASSWORD` in `.env`
- [ ] Change `GRAFANA_PASSWORD` in `.env`
- [ ] For public demos, consider a read-only API via `config.yaml` (`fhir.interactions.*`) and restrict resource types via `fhir.capability_statement.supported_resources`
- [ ] Set `BIND_ADDRESS=127.0.0.1` or use reverse proxy
- [ ] Enable TLS/HTTPS (use nginx/traefik reverse proxy)
- [ ] Review and adjust `WORKER_CONCURRENCY`
- [ ] Set up regular database backups
- [ ] Configure monitoring alerts (see `monitoring/README.md`)

### Reverse Proxy Example (nginx)

```nginx
server {
    listen 443 ssl http2;
    server_name fhir.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location /fhir/ {
        proxy_pass http://localhost:8080/fhir/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## Files Included

```
.
├── README.md                    # This file
├── .env.example                 # Configuration template
├── compose.yaml                 # Base services (required)
├── compose.monitoring.yaml      # Monitoring stack (optional)
├── config.yaml                  # Server configuration
└── monitoring/                  # Monitoring configs
    ├── README.md                # Monitoring documentation
    ├── prometheus.yml.template  # Metrics collection config
    ├── grafana/                 # Dashboards and datasources
    │   ├── dashboards/          # Pre-configured dashboards
    │   └── provisioning/        # Auto-configuration
    └── ...                      # Other monitoring configs
```

## Upgrading

```bash
# Pull new images
docker compose pull

# Restart services
docker compose up -d

# Check logs
docker compose logs -f
```

Database migrations run automatically on startup.

## Troubleshooting

### Server won't start

Check logs:

```bash
docker compose logs fhir-server
```

Common issues:

- Database not ready: Wait 30s, server auto-retries
- Port already in use: Change `FHIR_SERVER_PORT` in `.env`
- Database connection error: Check `POSTGRES_*` settings in `.env`

### No metrics in Grafana

1. Make some requests to generate metrics:

   ```bash
   curl http://localhost:8080/fhir/metadata
   ```

2. Check Prometheus targets: http://localhost:9090/targets

   - Should show `fhir-server` as UP (green)

3. Check server metrics endpoint:
   ```bash
   curl http://localhost:8080/metrics
   ```

See `monitoring/README.md` for detailed troubleshooting.

### Performance issues

1. Check monitoring dashboards (if enabled)
2. Scale workers: `docker compose up -d --scale fhir-worker=3`
3. Increase `WORKER_CONCURRENCY` in `.env`
4. Check database performance in Grafana

## Fly.io Deployment

Deploy to Fly.io with a single script. This is ideal for cloud deployments with managed infrastructure.

### Prerequisites

1. Install the Fly CLI: https://fly.io/docs/hands-on/install-flyctl/
2. Login: `fly auth login`
3. Create a Fly Postgres database (if not exists):
   ```bash
   fly postgres create --name zunder-db --region fra
   ```

### One-Command Deployment

From the `server/` directory, run:

```bash
./deploy.sh
```

This script will:

1. Build Docker image locally for linux/amd64
2. Authenticate with Fly registry
3. Push image to Fly
4. Deploy to your Fly app

### Manual Deployment Steps

If you prefer to run steps manually:

```bash
# 1. Build for linux (from macOS)
docker build --platform linux/amd64 -t registry.fly.io/zunder:latest .

# 2. Authenticate
fly auth docker

# 3. Push image
docker push registry.fly.io/zunder:latest

# 4. Deploy
fly deploy --image registry.fly.io/zunder:latest
```

### Initial Setup (One-Time)

After first deployment, configure the database connection:

```bash
# Attach Postgres database
fly postgres attach zunder-db --app zunder

# Verify DATABASE_URL is set
fly secrets list

# If not set or needs updating, set manually (adjust port to 5433 for Fly Postgres)
fly secrets set DATABASE_URL="postgres://user:pass@zunder-db.flycast:5433/fhir?sslmode=disable"
```

### Disable FHIR Package Auto-Loading (Recommended for 1-2GB RAM)

To avoid out-of-memory issues during startup, disable automatic FHIR package installation:

```bash
fly secrets set \
  FHIR__FHIR__DEFAULT_PACKAGES__CORE__INSTALL=false \
  FHIR__FHIR__DEFAULT_PACKAGES__EXTENSIONS__INSTALL=false \
  FHIR__FHIR__DEFAULT_PACKAGES__TERMINOLOGY__INSTALL=false
```

### Scaling

```bash
# Check current status
fly status

# Scale memory (if getting OOM errors)
fly scale memory 2048  # 2GB
fly scale memory 4096  # 4GB (requires 2 CPUs)

# Scale CPUs
fly scale vm shared-cpu-2x  # 2 CPUs, 4GB RAM
```

### Monitoring

```bash
# View logs
fly logs

# Check app status
fly status

# SSH into the machine
fly ssh console

# Check database status
fly postgres list
```

### Troubleshooting

**Database keeps stopping:**

- Fly auto-stops idle Postgres databases by default
- Start manually: `fly machine start <machine-id> --app zunder-db`
- Or configure to always run (increases costs)

**Out of memory during package loading:**

- Disable FHIR packages (see above)
- Or scale to at least 4GB RAM

**Build fails with CPU limits:**

- Use the `deploy.sh` script which builds locally instead of remote builder
- Free tier has a 4 CPU core limit for remote builds

## Support

- **Documentation**: https://github.com/thalamiq/zunder
- **Issues**: https://github.com/thalamiq/zunder/issues
- **FHIR R4 Spec**: https://hl7.org/fhir/R4/

## License

See LICENSE file included in the distribution.
