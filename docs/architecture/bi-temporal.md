# Bi-Temporal Model

QilbeeDB implements a bi-temporal data model, tracking both when events occurred (event time) and when they were recorded (transaction time).

## The Two Time Dimensions

### Event Time (Valid Time)
When an event actually occurred in the real world.

### Transaction Time (System Time)
When the database recorded the event.

## Why Bi-Temporal?

1. **Historical Queries**: "What did we know at time X?"
2. **Corrections**: Update past memories without losing history
3. **Audit Trail**: Track when information was learned
4. **Time Travel**: Query the database as it was at any point

## Data Structure

```rust
pub struct Episode {
    pub id: EpisodeId,
    pub content: String,
    
    // Event time dimension
    pub event_time: DateTime<Utc>,
    pub event_end_time: Option<DateTime<Utc>>,
    
    // Transaction time dimension
    pub transaction_time: DateTime<Utc>,
    pub transaction_end_time: Option<DateTime<Utc>>,
}
```

## Use Cases

### 1. Historical Queries

Query what the database looked like at any point in time.

### 2. Corrections Without Data Loss

Update past data while preserving history.

### 3. Audit Trail

Track all changes for compliance.

### 4. Time Travel Debugging

Debug issues by examining historical state.

## Example Usage

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode
from datetime import datetime

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('assistant')

# Store historical event
episode = Episode.conversation(
    'assistant',
    'User question',
    'Agent response',
    event_time=datetime(2024, 1, 1, 12, 0, 0)
)
memory.store_episode(episode)

# Query historical state
as_of = datetime(2024, 1, 1, 0, 0, 0)
historical = memory.recall(as_of_transaction_time=as_of, limit=100)
```

## Next Steps

- Explore [Memory Engine](memory-engine.md)
- Learn about [Agent Memory](../agent-memory/overview.md)
- Review [Memory API](../api/memory-api.md)
