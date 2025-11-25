# Performance Tuning

Optimize QilbeeDB performance for your workload.

## Hardware Recommendations

### Development
- CPU: 2 cores
- RAM: 4 GB
- Storage: 20 GB SSD

### Production
- CPU: 8+ cores
- RAM: 32+ GB
- Storage: 500+ GB NVMe SSD

## Configuration

### Storage Engine

```toml
[storage]
block_cache_size = "8GB"      # 25% of RAM
compression = "lz4"           # Fast compression
max_open_files = 10000        # Increase for large datasets

[storage.rocksdb]
max_background_jobs = 8       # Match CPU cores
write_buffer_size = "128MB"   # Larger for write-heavy workloads
max_write_buffer_number = 4
```

### Memory Settings

```toml
[memory]
query_cache_size = "2GB"      # Cache frequently used queries
result_cache_size = "1GB"     # Cache query results
```

### Connection Pool

```toml
[server]
max_connections = 1000        # Concurrent connections
connection_timeout_ms = 30000
```

## Indexing

Create indexes for frequently queried properties:

```cypher
-- Index on user email for fast lookups
CREATE INDEX ON :User(email)

-- Composite index
CREATE INDEX ON :Post(author_id, timestamp)
```

Monitor index usage:
```bash
curl http://localhost:7474/admin/indexes
```

## Query Optimization

### Use Parameters

```python
# Good: Plan cached
graph.query("MATCH (u:User) WHERE u.age > $age RETURN u", {'age': 25})

# Bad: Reparse every time
graph.query("MATCH (u:User) WHERE u.age > 25 RETURN u")
```

### Limit Results

```cypher
-- Always use LIMIT
MATCH (u:User) RETURN u LIMIT 100
```

### Use EXPLAIN

```cypher
EXPLAIN MATCH (u:User)-[:KNOWS*2..3]->(f) RETURN f
```

### Avoid Cartesian Products

```cypher
-- Bad: Cartesian product
MATCH (u:User), (p:Post) WHERE u.id = p.author_id

-- Good: Direct relationship
MATCH (u:User)-[:POSTED]->(p:Post)
```

## Monitoring

Track key metrics:

```bash
# Query performance
curl http://localhost:7474/admin/slow-queries

# Storage usage
curl http://localhost:7474/admin/stats
```

## Benchmarking

Run benchmarks to establish baselines:

```bash
# Create test data
python scripts/generate-test-data.py --nodes=1000000

# Run benchmark
python scripts/benchmark.py --queries=10000
```

## Scaling

### Vertical Scaling
- Add more RAM for larger caches
- Add more CPU cores for parallel queries
- Use faster storage (NVMe)

### Horizontal Scaling
- Read replicas (coming soon)
- Sharding (coming soon)

## Common Issues

### Slow Queries
- Add indexes
- Use LIMIT
- Optimize query patterns

### High Memory Usage
- Reduce cache sizes
- Limit concurrent queries
- Add more RAM

### Storage Growth
- Enable compression
- Archive old data
- Compact database

## Best Practices

1. **Monitor Performance**
   - Track query latency
   - Monitor resource usage
   - Set up alerts

2. **Regular Maintenance**
   - Compact database monthly
   - Update statistics
   - Review slow queries

3. **Test Changes**
   - Benchmark before/after
   - Test on staging first
   - Monitor production metrics

## Next Steps

- Set up [Monitoring](monitoring.md)
- Configure [Backups](backup.md)
- Review [Architecture](../architecture/overview.md)
