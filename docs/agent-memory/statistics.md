# Memory Statistics

QilbeeDB provides comprehensive statistics for monitoring agent memory usage, performance, and health.

## Overview

Memory statistics help you:
- Monitor memory growth
- Track relevance trends
- Identify memory patterns
- Optimize consolidation and forgetting

## Getting Statistics

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('my_agent')

# Get memory statistics
stats = memory.get_statistics()

print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance}")
print(f"Oldest episode: {stats.oldest_episode}")
print(f"Newest episode: {stats.newest_episode}")
```

## Available Statistics

*This page is under development. Full documentation coming soon.*

Statistics include:
- Total episode count
- Average relevance score
- Temporal range (oldest to newest)
- Episode type distribution
- Memory growth rate

## Next Steps

- Learn about [Episodes](episodes.md)
- Configure [Consolidation](consolidation.md)
- Understand [Forgetting](forgetting.md)
- Review [Agent Memory Overview](overview.md)
