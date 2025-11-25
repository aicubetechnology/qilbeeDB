# Memory API

API for agent memory operations.

## Agent Memory

### Initialize Memory

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('my_agent')
```

## Episode Storage

### Store Episode

```python
from qilbeedb.memory import Episode

# Conversation
conversation = Episode.conversation(
    agent_id='my_agent',
    user_input='What is 2+2?',
    agent_response='The answer is 4'
)
memory.store_episode(conversation)

# Observation
observation = Episode.observation(
    agent_id='my_agent',
    content='User seems frustrated'
)
memory.store_episode(observation)

# Action
action = Episode.action(
    agent_id='my_agent',
    action='Sent email',
    result='Email delivered successfully'
)
memory.store_episode(action)
```

## Memory Retrieval

### Recall Recent

```python
# Most recent episodes
recent = memory.recall(recency_hours=24, limit=10)

for episode in recent:
    print(f"{episode.event_time}: {episode.content}")
```

### Recall by Relevance

```python
# Most relevant episodes
relevant = memory.recall(
    min_relevance=0.7,
    limit=20,
    order_by='relevance'
)
```

### Search Content

```python
# Search by content
results = memory.recall(
    content_contains='order #12345',
    limit=10
)
```

### Time Range Query

```python
from datetime import datetime, timedelta

yesterday = datetime.now() - timedelta(days=1)
today = datetime.now()

# Episodes from time range
time_range = memory.recall(
    event_time_start=yesterday,
    event_time_end=today
)
```

## Memory Types

```python
# Store semantic memory
fact = Episode.action(
    'my_agent',
    'Learned fact',
    'Python 3.12 released October 2023',
    memory_type='semantic'
)
memory.store_episode(fact)

# Query semantic memory
facts = memory.recall(
    memory_type='semantic',
    content_contains='Python'
)
```

## Forgetting

```python
# Forget specific episode
memory.forget(episode_id=12345)

# Forget old, low-relevance episodes
memory.forget_old(
    older_than_days=365,
    max_relevance=0.2
)
```

## Statistics

```python
# Get memory statistics
stats = memory.statistics()
print(f"Total episodes: {stats['total_episodes']}")
print(f"Avg relevance: {stats['avg_relevance']}")
```

## Consolidation

```python
# Trigger consolidation
memory.consolidate(force=True)

# Configure consolidation
memory.configure(
    min_relevance_threshold=0.1,
    max_memory_size=1000000,
    forgetting_enabled=True
)
```

## Next Steps

- Learn about [Memory Engine](../architecture/memory-engine.md)
- Explore [Agent Memory](../agent-memory/overview.md)
- See [AI Agents Use Case](../use-cases/ai-agents.md)
