# Active Forgetting

Active forgetting is QilbeeDB's mechanism for intelligently pruning low-relevance memories to maintain system performance and focus on important information.

## Overview

*This page is under development. Full documentation coming soon.*

## Key Concepts

- Relevance decay over time
- Configurable forgetting thresholds
- Protected memories that never decay
- Automatic memory pruning

## Configuration

Configure forgetting behavior in your QilbeeDB configuration:

```toml
[memory]
enable_auto_forgetting = true
min_relevance_threshold = 0.1
relevance_decay_days = 30
```

## Next Steps

- Review [Memory Statistics](statistics.md)
- Read [Memory Consolidation](consolidation.md)
- Learn about [Episodes](episodes.md)
