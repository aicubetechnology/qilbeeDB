# Monitoring

Monitor QilbeeDB performance and health.

## Health Check

```bash
# HTTP health endpoint
curl http://localhost:7474/health
```

Response:
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600
}
```

## Metrics

### System Metrics

```bash
# Get system statistics
curl http://localhost:7474/admin/stats
```

Response:
```json
{
  "storage": {
    "totalSizeBytes": 1073741824,
    "nodeCount": 1000000,
    "relationshipCount": 5000000
  },
  "memory": {
    "usedBytes": 536870912,
    "availableBytes": 8589934592
  },
  "queries": {
    "totalExecuted": 50000,
    "avgExecutionTimeMs": 25
  }
}
```

### Query Performance

```bash
# Get slow query log
curl http://localhost:7474/admin/slow-queries
```

## Logging

Configure logging in `config.toml`:

```toml
[logging]
level = "info"  # trace, debug, info, warn, error
format = "json"  # json, text
output = "/var/log/qilbeedb/qilbee.log"
```

View logs:
```bash
# Docker logs
docker logs -f qilbeedb

# File logs
tail -f /var/log/qilbeedb/qilbee.log
```

## Prometheus Integration

Expose metrics for Prometheus:

```toml
[metrics]
enabled = true
address = "0.0.0.0:9090"
```

Prometheus config:
```yaml
scrape_configs:
  - job_name: 'qilbeedb'
    static_configs:
      - targets: ['localhost:9090']
```

## Grafana Dashboard

Import QilbeeDB dashboard:
- Query throughput
- Response times
- Storage usage
- Memory usage
- Connection count

## Alerts

Set up alerts for:
- High query latency (>100ms avg)
- Storage near capacity (>80%)
- High memory usage (>90%)
- Connection pool exhaustion

## Next Steps

- Configure [Deployment](deployment.md)
- Set up [Backups](backup.md)
- Optimize [Performance](performance.md)
