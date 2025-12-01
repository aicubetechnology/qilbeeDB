# Memory Engine

QilbeeDB's memory engine provides bi-temporal memory storage for AI agents, tracking both when events occurred and when they were recorded. All memories are automatically persisted to RocksDB, ensuring durability across server restarts.

## Architecture

```
Agent Memory Interface
  ↓
Memory Types (Episodic | Semantic | Procedural | Factual)
  ↓
Bi-Temporal Storage (Event Time | Transaction Time)
  ↓
Consolidation Engine (Short-term → Long-term)
  ↓
RocksDB Persistence (WAL | LZ4 Compression)
```

## Persistence Layer

The memory engine uses RocksDB as its persistence backend:

- **Write-Ahead Logging (WAL)**: Ensures durability and crash recovery
- **LZ4 Compression**: Reduces storage footprint
- **Agent Isolation**: Episodes stored in separate namespaces per agent
- **Automatic Recovery**: Memories available immediately after restart

## Memory Types

### Episodic Memory
Personal experiences and events (conversations, observations).

### Semantic Memory
General knowledge and facts.

### Procedural Memory
How-to knowledge and procedures.

### Factual Memory
Timestamped facts about entities.

## Bi-Temporal Model

Every memory has two timestamps:

- **Event Time**: When the event actually occurred
- **Transaction Time**: When it was recorded in the database

This enables:
- Historical queries
- Corrections without data loss
- Audit trail
- Time-travel debugging

## Consolidation

Memories automatically consolidate from short-term to long-term based on:
- Relevance score
- Access frequency
- Time since creation
- Relationships to other memories

## Relevance Scoring

Each memory has a dynamic relevance score based on:
1. **Recency**: Recent memories score higher
2. **Access Frequency**: Frequently accessed memories score higher
3. **Importance**: Manually set importance level
4. **Connections**: Memories connected to many others score higher

## Example Usage

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('assistant')

# Store conversation
episode = Episode.conversation(
    'assistant',
    'What is 2+2?',
    'The answer is 4'
)
memory.store_episode(episode)

# Recall recent episodes
recent = memory.recall(recency_hours=24, limit=10)
```

## Next Steps

- Learn about [Bi-Temporal Model](bi-temporal.md)
- Explore [Agent Memory](../agent-memory/overview.md)
- Configure [Memory Persistence](../agent-memory/persistence.md)
- Review [Memory API](../api/memory-api.md)
