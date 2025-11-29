# Memory Persistence

QilbeeDB provides enterprise-grade memory persistence using RocksDB as the storage backend. All agent memories are automatically persisted to disk, ensuring durability across server restarts.

## Overview

Memory persistence in QilbeeDB is handled transparently by the server. When you store episodes through the Python SDK or HTTP API, they are automatically:

1. **Written to RocksDB** - Stored in an efficient LSM-tree structure
2. **Protected by WAL** - Write-ahead logging ensures durability
3. **Compressed with LZ4** - Reduces storage footprint
4. **Automatically recovered** - Available immediately after server restart

```
┌─────────────────────────────────────────┐
│     Python SDK / HTTP API               │
│   memory.store_episode(episode)         │
└─────────────────────────────────────────┘
           ↓ Automatic Persistence
┌─────────────────────────────────────────┐
│     Memory Storage Layer                │
│   • Episode serialization               │
│   • Agent-scoped storage                │
│   • Unique ID generation                │
└─────────────────────────────────────────┘
           ↓
┌─────────────────────────────────────────┐
│     RocksDB Backend                     │
│   • Write-Ahead Log (WAL)               │
│   • LZ4 Compression                     │
│   • Automatic Recovery                  │
└─────────────────────────────────────────┘
```

## Key Features

### Automatic Durability

Episodes are persisted automatically when stored:

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
memory = db.agent_memory("my-agent")

# This episode is automatically persisted to disk
episode = Episode.conversation(
    "my-agent",
    "What is QilbeeDB?",
    "QilbeeDB is a graph database with agent memory..."
)
episode_id = memory.store_episode(episode)

# Episode survives server restart
# No explicit save or flush required
```

### Write-Ahead Logging (WAL)

QilbeeDB uses RocksDB's write-ahead logging for durability:

- **Crash Recovery**: Episodes are recoverable even after unexpected shutdowns
- **Transaction Safety**: Writes are atomic and consistent
- **Configurable Sync**: Balance between performance and durability

### Compression

All stored episodes are compressed using LZ4:

- **Fast Compression**: Minimal overhead on write operations
- **Reduced Storage**: Typically 50-70% size reduction
- **Transparent**: No application changes required

### Agent Isolation

Episodes are stored in separate namespaces per agent:

```python
# Each agent has isolated storage
sales_memory = db.agent_memory("sales-agent")
support_memory = db.agent_memory("support-agent")

# Episodes are stored separately
sales_memory.store_episode(Episode.conversation(
    "sales-agent", "Pricing inquiry", "Our plans start at..."
))

support_memory.store_episode(Episode.conversation(
    "support-agent", "How do I reset?", "Click forgot password..."
))
```

## Server Configuration

Memory persistence is configured on the server side. The Python SDK automatically benefits from these settings without any code changes.

### Storage Path

```toml
[storage]
data_path = "/var/lib/qilbeedb/data"
```

### WAL Configuration

```toml
[storage]
enable_wal = true           # Enable write-ahead logging
sync_writes = true          # Sync each write (safer, slower)
wal_sync_interval_ms = 1000 # Sync interval for async writes
```

### Compression Settings

```toml
[storage]
enable_compression = true
compression_type = "lz4"    # Options: none, lz4, snappy, zstd
```

## Using Persistence in Python

### Basic Usage

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

# Connect to QilbeeDB
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")

# Get memory manager for an agent
memory = db.agent_memory("customer-service-bot")

# Store episodes (automatically persisted)
episode = Episode.conversation(
    "customer-service-bot",
    "I need help with my order",
    "I'd be happy to help! What's your order number?"
)
episode_id = memory.store_episode(episode)
print(f"Stored episode: {episode_id}")

# Episodes persist across sessions
# Restart your application or the server - data remains
```

### Verifying Persistence

```python
# After server restart, episodes are still available
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
memory = db.agent_memory("customer-service-bot")

# Get statistics to verify data persisted
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance}")

# Retrieve recent episodes
recent = memory.get_recent_episodes(limit=10)
for ep in recent:
    print(f"[{ep.episode_type}] {ep.content}")
```

### Searching Persisted Episodes

```python
# Search through persisted episodes
results = memory.search_episodes("order number", limit=5)
for episode in results:
    print(f"Found: {episode.content}")
```

### Deleting Episodes

```python
# Delete a specific episode
deleted = memory.delete_episode(episode_id)
if deleted:
    print("Episode permanently removed from storage")

# Clear all episodes for an agent
memory.clear()
print("All episodes cleared for this agent")
```

## Best Practices

### 1. Use Meaningful Agent IDs

Agent IDs serve as storage namespaces. Use consistent, descriptive names:

```python
# Good: Descriptive and consistent
memory = db.agent_memory("customer-support-v2")
memory = db.agent_memory("sales-assistant-prod")

# Avoid: Generic or inconsistent names
memory = db.agent_memory("agent1")
memory = db.agent_memory("test")
```

### 2. Handle Connection Errors

Implement retry logic for robustness:

```python
import time
from qilbeedb.exceptions import MemoryError

def store_with_retry(memory, episode, max_retries=3):
    for attempt in range(max_retries):
        try:
            return memory.store_episode(episode)
        except MemoryError as e:
            if attempt == max_retries - 1:
                raise
            time.sleep(1 * (attempt + 1))
```

### 3. Monitor Storage Statistics

Regularly check storage health:

```python
stats = memory.get_statistics()

# Monitor episode count
if stats.total_episodes > 100000:
    print("Warning: High episode count, consider consolidation")

# Check relevance distribution
if stats.avg_relevance < 0.3:
    print("Many low-relevance episodes, consider forgetting")
```

### 4. Use Appropriate Episode Types

Choose the right episode type for your data:

```python
# Conversations: User interactions
Episode.conversation(agent_id, user_input, response)

# Observations: Environmental data
Episode.observation(agent_id, "CPU usage at 85%")

# Actions: Agent decisions and outcomes
Episode.action(agent_id, "Scaled to 4 instances", "Latency reduced")
```

## Performance Considerations

### Write Performance

- Episodes are written asynchronously by default
- WAL ensures durability without blocking
- LZ4 compression adds minimal overhead

### Read Performance

- Recent episodes are cached in memory
- RocksDB bloom filters speed up lookups
- Agent-scoped storage enables efficient queries

### Storage Efficiency

- LZ4 compression reduces disk usage
- Automatic compaction reclaims space
- Episode consolidation reduces redundancy

## Troubleshooting

### Episodes Not Persisting

1. Check server logs for storage errors
2. Verify disk space is available
3. Ensure proper permissions on data directory
4. Check WAL configuration

### Slow Write Performance

1. Consider async WAL sync
2. Check disk I/O utilization
3. Review compression settings
4. Monitor compaction status

### Recovery After Crash

1. QilbeeDB automatically recovers from WAL
2. Check server logs for recovery status
3. Verify episode counts after restart
4. Report any data loss issues

## Related Documentation

- [Agent Memory Overview](overview.md)
- [Episodes](episodes.md)
- [Memory Types](memory-types.md)
- [Storage Engine](../architecture/storage.md)
- [Configuration](../getting-started/configuration.md)
- [Memory API Reference](../api/memory-api.md)
