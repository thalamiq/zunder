# FHIR Server Monitoring Stack

**Production-ready monitoring** for your FHIR server with Prometheus + Grafana + pre-configured dashboards.

## 🚀 Quick Start (30 seconds)

### 1. Start the stack

```bash
cd docker
./start.sh monitoring  # or: docker-compose -f docker-compose.yaml -f docker-compose.monitoring.yaml up -d
```

### 2. Open Grafana

Visit http://localhost:3000 and login:
- Username: `admin`
- Password: `admin`

### 3. View dashboards

Click **Dashboards** in the left menu. You'll see:
- ✅ **FHIR Server - Overview** (request rates, latencies, errors)
- ✅ **FHIR Server - Operations** (CRUD ops, searches, resource types)
- ✅ **PostgreSQL - FHIR Database** (connections, queries, cache)

**That's it!** All dashboards are pre-configured and working.

## 🔗 Quick Links

- **Grafana Dashboards**: http://localhost:3000
- **Prometheus Metrics**: http://localhost:9090
- **FHIR Server Metrics**: http://localhost:8080/metrics
- **PostgreSQL Exporter**: http://localhost:9187/metrics

## 📊 Dashboard Details

### 1. FHIR Server - Overview
Real-time HTTP metrics and system health:
- Request rate, latency percentiles (p50, p95, p99)
- Error rates (4xx, 5xx) by status code
- Active requests and in-flight operations
- Database connection pool status
- Job queue backlog (pending/running jobs)
- Request breakdown by endpoint

### 2. FHIR Server - Operations
Deep dive into FHIR operations:
- CRUD operations (create, read, update, delete, search)
- Operations grouped by resource type (Patient, Observation, etc.)
- Operation latency by type and resource
- Search performance and result count distributions
- Error analysis by resource and operation type

### 3. PostgreSQL - FHIR Database
Database performance monitoring:
- Active connections and cache hit ratio
- Transaction rates (commits/rollbacks) and database size
- Row operations (fetched, inserted, updated, deleted)
- I/O performance and block access patterns
- Connection pool status from the application

## 📊 Available Metrics

### FHIR Server Application Metrics

Exposed at http://localhost:8080/metrics - automatically tracked by middleware:

**HTTP Request Metrics:**
- `fhir_http_requests_total` - Total HTTP requests (by method, path, status)
- `fhir_http_request_duration_seconds` - Request latency histogram
- `fhir_http_requests_in_flight` - Current active requests
- `fhir_http_request_size_bytes` - Request body size
- `fhir_http_response_size_bytes` - Response body size

**FHIR Operation Metrics:**
- `fhir_operations_total` - FHIR operations by resource type and operation (create, read, update, delete, search)
- `fhir_operation_duration_seconds` - Operation duration histogram
- `fhir_search_total` - Search operations by resource type
- `fhir_search_results` - Number of results per search
- `fhir_batch_operations_total` - Batch/transaction operations
- `fhir_batch_entries` - Entries per batch

**Database Metrics:**
- `fhir_db_connections_active` - Active database connections
- `fhir_db_connections_idle` - Idle database connections
- `fhir_db_query_duration_seconds` - Query duration histogram
- `fhir_db_query_errors_total` - Database errors

**Job Queue Metrics:**
- `fhir_jobs_queue_size` - Jobs in queue by status (pending, running, failed)
- `fhir_jobs_enqueued_total` - Total jobs enqueued
- `fhir_jobs_completed_total` - Total jobs completed
- `fhir_job_duration_seconds` - Job execution duration

**Indexing Metrics:**
- `fhir_indexing_resources_total` - Resources indexed
- `fhir_indexing_duration_seconds` - Indexing duration
- `fhir_indexing_parameters_count` - Search parameters indexed per resource

**Resource Metrics:**
- `fhir_resources_total` - Total resources by type
- `fhir_resource_versions` - Version count per resource
- `fhir_server_info` - Server version and FHIR version

### Standard PostgreSQL Metrics (via postgres_exporter)

- **Connection stats**: `pg_stat_database_*`
- **Query performance**: `pg_stat_statements_*`
- **Locks**: `pg_locks_count`
- **Replication**: `pg_replication_lag`
- **Cache hit ratio**: `pg_stat_database_blks_hit`
- **Transaction rates**: `pg_stat_database_xact_commit`

### FHIR-Specific Custom Metrics

Defined in `postgres-exporter-queries.yaml`:

1. **Search Parameter Statistics** (`pg_fhir_search_stats`)
   - Row counts per search table (search_string, search_token, etc.)
   - Table sizes

2. **Resource Statistics** (`pg_fhir_resources`)
   - Resource counts by type
   - Version statistics
   - Growth metrics

3. **Query Performance** (`pg_stat_statements`)
   - Top 50 slowest queries
   - Execution time statistics
   - Row counts

4. **Table Health** (`pg_table_stats`)
   - Dead tuple counts (bloat indicator)
   - Vacuum statistics
   - Index sizes

## 🔍 Debugging Workflows

### "Why is my FHIR server slow?"

1. **Check Application Metrics** (Prometheus: http://localhost:9090):
   ```promql
   # Request rate
   rate(fhir_http_requests_total[5m])

   # Average latency by endpoint
   rate(fhir_http_request_duration_seconds_sum[5m]) / rate(fhir_http_request_duration_seconds_count[5m])

   # 95th percentile latency
   histogram_quantile(0.95, rate(fhir_http_request_duration_seconds_bucket[5m]))

   # Error rate
   rate(fhir_http_requests_total{status=~"5.."}[5m])
   ```

2. **Check Database Performance**:
   ```promql
   # Database transaction rate
   rate(pg_stat_database_xact_commit[5m])

   # Active connections
   fhir_db_connections_active
   ```

3. **Check FHIR Operations**:
   ```promql
   # Operations by resource type
   sum by (resource_type, operation) (rate(fhir_operations_total[5m]))

   # Slow operations
   topk(10, fhir_operation_duration_seconds{quantile="0.99"})
   ```

4. **Find Slow Queries** (direct SQL):
   ```sql
   SELECT query, calls, mean_exec_time, total_exec_time
   FROM pg_stat_statements
   ORDER BY mean_exec_time DESC
   LIMIT 10;
   ```

### "Database is using too much disk"

1. Check table sizes in Grafana
2. Run vacuum analysis:
   ```sql
   SELECT
     schemaname,
     tablename,
     n_dead_tup,
     last_autovacuum
   FROM pg_stat_user_tables
   WHERE n_dead_tup > 10000
   ORDER BY n_dead_tup DESC;
   ```

### "Too many connections"

1. Check in Grafana: Active connections panel
2. Find connection sources:
   ```sql
   SELECT
     state,
     count(*),
     application_name
   FROM pg_stat_activity
   GROUP BY state, application_name
   ORDER BY count DESC;
   ```

## 🎯 Recommended Alerts (coming soon)

Add these to `prometheus/alerts/`:

```yaml
# FHIR Server Alerts
- alert: FHIRServerDown
  expr: up{job="fhir-server"} == 0
  for: 1m
  annotations:
    summary: "FHIR server is down"

- alert: HighErrorRate
  expr: rate(fhir_http_requests_total{status=~"5.."}[5m]) > 0.05
  for: 5m
  annotations:
    summary: "High error rate (>5%)"

- alert: HighLatency
  expr: histogram_quantile(0.95, rate(fhir_http_request_duration_seconds_bucket[5m])) > 2
  for: 5m
  annotations:
    summary: "95th percentile latency > 2s"

- alert: JobQueueBacklog
  expr: fhir_jobs_queue_size{status="pending"} > 1000
  for: 10m
  annotations:
    summary: "Job queue backlog > 1000"

# Database Alerts
- alert: PostgreSQLDown
  expr: pg_up == 0
  for: 1m

- alert: HighConnectionCount
  expr: pg_stat_database_numbackends > 80
  for: 5m

- alert: SlowQueries
  expr: rate(pg_stat_statements_mean_exec_time[5m]) > 1000
  for: 5m

- alert: DeadTuples
  expr: pg_stat_user_tables_n_dead_tup > 10000
  for: 10m
```

## 📁 File Structure

```
monitoring/
├── README.md                                    # This file
├── init-monitoring.sql                          # PostgreSQL initialization
├── prometheus.yml                               # Prometheus configuration
├── postgres-exporter-queries.yaml              # Custom FHIR metrics
└── grafana/
    ├── provisioning/
    │   ├── datasources/prometheus.yml          # Auto-configure Prometheus
    │   └── dashboards/default.yml              # Auto-load dashboards
    └── dashboards/                             # Put custom .json dashboards here
```

## 📈 Advanced: Import Additional Dashboards

Want more dashboards? Browse and import from [Grafana.com](https://grafana.com/grafana/dashboards/):

1. In Grafana, navigate to **Dashboards → Import**
2. Enter a Dashboard ID (e.g., **455** for PostgreSQL Overview)
3. Click **Load**
4. Select **Prometheus** as the data source
5. Click **Import**

**Popular PostgreSQL dashboards:**
- **455** - PostgreSQL Database (alternative view)
- **3300** - PostgreSQL Dashboard (extended metrics)
- **14114** - PostgreSQL Database Monitoring

## 🔧 Customization

### Customize existing dashboards

All dashboards are editable:
1. Open any dashboard
2. Click the gear icon ⚙️ (Dashboard settings)
3. Modify panels, add new ones, or adjust queries
4. Click **Save dashboard**

Changes are persisted in the Grafana database (not the JSON files).

### Export modified dashboards

To save your changes permanently:
1. Click **Share → Export → Save to file**
2. Save JSON to `monitoring/grafana/dashboards/`
3. Rename the file to avoid conflicts (e.g., `my-custom-dashboard.json`)
4. Restart Grafana: `docker-compose restart grafana`

### Add a custom metric

Edit `postgres-exporter-queries.yaml`:

```yaml
my_custom_metric:
  query: "SELECT COUNT(*) as count FROM my_table WHERE condition"
  metrics:
    - count:
        usage: "GAUGE"
        description: "My custom count"
```


### Change retention period

Edit `docker-compose.yaml`:

```yaml
prometheus:
  command:
    - "--storage.tsdb.retention.time=90d"  # Change from 30d to 90d
```

## 🐛 Troubleshooting

### No metrics showing in dashboards

**Quick diagnostic:**
```bash
cd docker/monitoring
./debug-prometheus.sh
```

**Common causes:**
1. **No requests made yet** - Metrics are generated when requests are made
   ```bash
   # Generate test metrics
   ./generate-test-metrics.sh
   ```

2. **Prometheus not scraping** - Check targets: http://localhost:9090/targets
   - Should show `fhir-server` target as **UP** (green)
   - If **DOWN** (red), see [DEBUGGING.md](./DEBUGGING.md)

3. **Metrics endpoint not accessible**
   ```bash
   # Test metrics endpoint
   curl http://localhost:8080/metrics
   
   # Should return Prometheus-formatted metrics
   # If empty, make some requests first:
   curl http://localhost:8080/fhir/metadata
   ```

**Full debugging guide:** See [DEBUGGING.md](./DEBUGGING.md)

### Prometheus not scraping metrics

Check targets: http://localhost:9090/targets

All should be **UP**. If down:
```bash
docker-compose logs postgres-exporter
```

### Grafana can't connect to Prometheus

```bash
docker-compose exec grafana curl http://prometheus:9090/api/v1/status/config
```

Should return Prometheus config. If not, check networking:
```bash
docker network inspect fhir-network
```

### pg_stat_statements not enabled

Connect to PostgreSQL:
```bash
docker-compose exec db psql -U fhir
```

Check extension:
```sql
\dx
SELECT * FROM pg_stat_statements LIMIT 1;
```

If error, restart PostgreSQL container to apply `shared_preload_libraries`:
```bash
docker-compose restart db
```

## 📚 Resources

- [Official postgres_exporter](https://github.com/prometheus-community/postgres_exporter)
- [Grafana Dashboard 9628](https://grafana.com/grafana/dashboards/9628)
- [PostgreSQL monitoring guide](https://www.postgresql.org/docs/current/monitoring-stats.html)
- [Prometheus query examples](https://prometheus.io/docs/prometheus/latest/querying/examples/)
