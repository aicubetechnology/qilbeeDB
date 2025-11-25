# Multi-Agent Systems

QilbeeDB excels at coordinating multiple AI agents with separate memory spaces while enabling shared knowledge graphs.

## Separate Agent Memories

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")

# Each agent has isolated memory
sales_agent = db.agent_memory('sales_agent')
support_agent = db.agent_memory('support_agent')

# Sales interactions
sales_agent.store_episode(Episode.conversation(
    'sales_agent',
    'Tell me about pricing',
    'Our plans start at $99/month...'
))

# Support interactions
support_agent.store_episode(Episode.conversation(
    'support_agent',
    'I need technical help',
    'Let me assist you with that...'
))
```

## Shared Knowledge

```python
# Shared knowledge graph accessible to all agents
shared_kb = db.graph('company_knowledge')

product = shared_kb.create_node(['Product'], {
    'name': 'Enterprise Plan',
    'price': 999
})

# All agents can query shared knowledge
results = shared_kb.query("""
    MATCH (p:Product)
    WHERE p.name CONTAINS 'Enterprise'
    RETURN p.name, p.price
""")
```

## Agent Coordination

```python
# Agents can reference each other's work
handoff = Episode.action(
    'support_agent',
    'Escalated to sales for upgrade discussion',
    'Customer interested in Enterprise features'
)
support_agent.store_episode(handoff)
```

## Next Steps

- Read [AI Agents](ai-agents.md) use case
- Explore [Agent Memory](../agent-memory/overview.md)
- Learn the [Python SDK](../client-libraries/python.md)
