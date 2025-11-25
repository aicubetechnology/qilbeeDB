# Configuration

QilbeeDB provides flexible configuration options through configuration files, environment variables, and command-line arguments. This guide covers all available settings and best practices.

## Configuration Methods

QilbeeDB supports three configuration methods (in order of precedence):

1. **Command-line arguments** (highest priority)
2. **Environment variables**
3. **Configuration file** (lowest priority)

## Configuration File

Create a `config.toml` file to configure QilbeeDB:

```toml
[server]
http_port = 7474
bolt_port = 7687
bind_address = "0.0.0.0"
max_connections = 1000
request_timeout_ms = 30000

[storage]
data_path = "/var/lib/qilbeedb/data"
enable_wal = true
sync_writes = true
wal_sync_interval_ms = 1000
max_open_files = 1000

[memory]
# Agent memory settings
consolidation_interval_hours = 24
relevance_decay_days = 30
enable_auto_forgetting = true
min_relevance_threshold = 0.1

[query]
# Query execution settings
max_query_time_ms = 60000
enable_query_cache = true
cache_size_mb = 512
vectorized_execution = true

[logging]
level = "info"  # debug, info, warn, error
format = "json"  # json or text
output = "stdout"  # stdout, stderr, or file path

[metrics]
enabled = true
prometheus_port = 9090
export_interval_seconds = 15

[security]
# Authentication and encryption
enable_auth = false
cert_file = "/etc/qilbeedb/cert.pem"
key_file = "/etc/qilbeedb/key.pem"
```

### Using Configuration File

```bash
# Specify configuration file
qilbeedb --config /path/to/config.toml

# Use default location (./config.toml)
qilbeedb
```

## Environment Variables

All configuration options can be set via environment variables:

### Server Settings

```bash
# HTTP and Bolt ports
export QILBEE_HTTP_PORT=7474
export QILBEE_BOLT_PORT=7687
export QILBEE_BIND_ADDRESS=0.0.0.0

# Connection limits
export QILBEE_MAX_CONNECTIONS=1000
export QILBEE_REQUEST_TIMEOUT_MS=30000
```

### Storage Settings

```bash
# Data storage path
export QILBEE_STORAGE_PATH=/var/lib/qilbeedb/data

# Write-ahead log
export QILBEE_ENABLE_WAL=true
export QILBEE_SYNC_WRITES=true
export QILBEE_WAL_SYNC_INTERVAL_MS=1000

# File handles
export QILBEE_MAX_OPEN_FILES=1000
```

### Memory Settings

```bash
# Agent memory configuration
export QILBEE_CONSOLIDATION_INTERVAL_HOURS=24
export QILBEE_RELEVANCE_DECAY_DAYS=30
export QILBEE_ENABLE_AUTO_FORGETTING=true
export QILBEE_MIN_RELEVANCE_THRESHOLD=0.1
```

### Query Settings

```bash
# Query execution limits
export QILBEE_MAX_QUERY_TIME_MS=60000

# Query cache
export QILBEE_ENABLE_QUERY_CACHE=true
export QILBEE_CACHE_SIZE_MB=512

# Execution mode
export QILBEE_VECTORIZED_EXECUTION=true
```

### Logging Settings

```bash
# Log level and format
export QILBEE_LOG_LEVEL=info
export QILBEE_LOG_FORMAT=json
export QILBEE_LOG_OUTPUT=stdout
```

### Metrics Settings

```bash
# Prometheus metrics
export QILBEE_ENABLE_METRICS=true
export QILBEE_PROMETHEUS_PORT=9090
export QILBEE_EXPORT_INTERVAL_SECONDS=15
```

### Security Settings

```bash
# Authentication
export QILBEE_ENABLE_AUTH=false

# TLS certificates
export QILBEE_CERT_FILE=/etc/qilbeedb/cert.pem
export QILBEE_KEY_FILE=/etc/qilbeedb/key.pem
```

## Command-Line Arguments

Override any setting using command-line flags:

```bash
# Server settings
qilbeedb --http-port 8080 --bolt-port 8687

# Storage settings
qilbeedb --data-path /custom/path --enable-wal

# Logging
qilbeedb --log-level debug --log-format text

# Multiple options
qilbeedb \
  --http-port 8080 \
  --data-path /data \
  --log-level debug \
  --enable-metrics
```

## Configuration Profiles

### Development Profile

Optimized for local development:

```toml
[server]
http_port = 7474
bolt_port = 7687
bind_address = "127.0.0.1"

[storage]
data_path = "./data"
sync_writes = false  # Better performance for dev

[logging]
level = "debug"
format = "text"

[metrics]
enabled = false  # Disable in dev
```

### Production Profile

Optimized for production deployments:

```toml
[server]
http_port = 7474
bolt_port = 7687
bind_address = "0.0.0.0"
max_connections = 5000
request_timeout_ms = 60000

[storage]
data_path = "/var/lib/qilbeedb/data"
enable_wal = true
sync_writes = true
wal_sync_interval_ms = 500
max_open_files = 10000

[memory]
consolidation_interval_hours = 12
enable_auto_forgetting = true

[query]
max_query_time_ms = 120000
enable_query_cache = true
cache_size_mb = 2048
vectorized_execution = true

[logging]
level = "info"
format = "json"
output = "/var/log/qilbeedb/qilbeedb.log"

[metrics]
enabled = true
prometheus_port = 9090

[security]
enable_auth = true
cert_file = "/etc/qilbeedb/cert.pem"
key_file = "/etc/qilbeedb/key.pem"
```

### High-Performance Profile

Optimized for maximum throughput:

```toml
[server]
max_connections = 10000
request_timeout_ms = 120000

[storage]
sync_writes = false  # Async writes for speed
wal_sync_interval_ms = 2000
max_open_files = 50000

[query]
enable_query_cache = true
cache_size_mb = 8192  # Large cache
vectorized_execution = true

[memory]
consolidation_interval_hours = 48  # Less frequent
```

## Docker Configuration

### Using Environment Variables

```bash
docker run -d \
  -e QILBEE_HTTP_PORT=7474 \
  -e QILBEE_BOLT_PORT=7687 \
  -e QILBEE_LOG_LEVEL=info \
  -e QILBEE_ENABLE_METRICS=true \
  -p 7474:7474 \
  -p 7687:7687 \
  -v qilbeedb-data:/data \
  qilbeedb/qilbeedb:latest
```

### Using Configuration File

```bash
docker run -d \
  -v /path/to/config.toml:/etc/qilbeedb/config.toml \
  -v qilbeedb-data:/data \
  -p 7474:7474 \
  -p 7687:7687 \
  qilbeedb/qilbeedb:latest \
  --config /etc/qilbeedb/config.toml
```

### Docker Compose

```yaml
version: '3.8'

services:
  qilbeedb:
    image: qilbeedb/qilbeedb:latest
    ports:
      - "7474:7474"
      - "7687:7687"
      - "9090:9090"  # Metrics
    volumes:
      - ./config.toml:/etc/qilbeedb/config.toml
      - qilbeedb-data:/data
      - qilbeedb-logs:/var/log/qilbeedb
    environment:
      - QILBEE_LOG_LEVEL=info
      - QILBEE_ENABLE_METRICS=true
    command: --config /etc/qilbeedb/config.toml

volumes:
  qilbeedb-data:
  qilbeedb-logs:
```

## Performance Tuning

### Memory Tuning

```toml
[query]
cache_size_mb = 4096  # Adjust based on available RAM

[memory]
# More aggressive consolidation for memory-constrained systems
consolidation_interval_hours = 6
min_relevance_threshold = 0.3
```

### Storage Tuning

```toml
[storage]
# SSD-optimized settings
sync_writes = false
wal_sync_interval_ms = 2000
max_open_files = 10000

# Enable compression for disk space
enable_compression = true
compression_type = "lz4"
```

### Query Optimization

```toml
[query]
# Increase cache for query-heavy workloads
enable_query_cache = true
cache_size_mb = 8192

# Enable vectorized execution
vectorized_execution = true

# Adjust timeout for complex queries
max_query_time_ms = 180000
```

## Monitoring Configuration

### Prometheus Metrics

```toml
[metrics]
enabled = true
prometheus_port = 9090
export_interval_seconds = 15

# Metrics to export
export_query_stats = true
export_storage_stats = true
export_memory_stats = true
```

Access metrics at: `http://localhost:9090/metrics`

### Logging Configuration

```toml
[logging]
level = "info"
format = "json"

# Structured logging fields
include_timestamp = true
include_thread_id = true
include_file_location = true

# Log rotation
output = "/var/log/qilbeedb/qilbeedb.log"
max_size_mb = 100
max_backups = 10
compress_backups = true
```

## Security Configuration

### Enabling Authentication

```toml
[security]
enable_auth = true
auth_provider = "builtin"  # or "ldap", "oauth"

# User database
users_file = "/etc/qilbeedb/users.json"
```

### TLS/SSL Configuration

```toml
[security]
enable_tls = true
cert_file = "/etc/qilbeedb/cert.pem"
key_file = "/etc/qilbeedb/key.pem"
ca_file = "/etc/qilbeedb/ca.pem"

# TLS version
min_tls_version = "1.2"
```

## Validation

Validate your configuration:

```bash
# Check configuration syntax
qilbeedb --config config.toml --validate

# Dry run to test settings
qilbeedb --config config.toml --dry-run
```

## Best Practices

1. **Use Configuration Files**: Keep production settings in version-controlled config files

2. **Environment-Specific Configs**: Maintain separate configs for dev, staging, and production

3. **Secure Sensitive Data**: Use environment variables for secrets and credentials

4. **Monitor Resource Usage**: Adjust `cache_size_mb` and `max_connections` based on actual usage

5. **Enable Metrics**: Always enable metrics in production for observability

6. **Regular Backups**: Configure WAL and sync settings for data durability

7. **Log Rotation**: Configure log rotation to prevent disk space issues

## Troubleshooting

### Configuration Not Loading

```bash
# Check config file syntax
qilbeedb --config config.toml --validate

# Verify file permissions
ls -la config.toml

# Check environment variables
env | grep QILBEE_
```

### Performance Issues

1. Increase `cache_size_mb` if you have available RAM
2. Disable `sync_writes` for better write performance (if data loss is acceptable)
3. Increase `max_open_files` for large databases
4. Enable `vectorized_execution` for query performance

### Memory Issues

1. Reduce `cache_size_mb`
2. Enable more aggressive `auto_forgetting`
3. Reduce `consolidation_interval_hours`
4. Lower `max_connections`

## Next Steps

- Learn about [Graph Operations](../graph-operations/nodes.md)
- Explore [Deployment Options](../operations/deployment.md)
- Set up [Monitoring](../operations/monitoring.md)
- Configure [Backup & Recovery](../operations/backup.md)
