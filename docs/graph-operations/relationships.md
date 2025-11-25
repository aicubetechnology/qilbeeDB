# Relationships

Relationships connect nodes in the graph and represent connections between entities.

*This page is under development. Full documentation coming soon.*

## Creating Relationships

```python
# Create relationship between nodes
friendship = graph.create_relationship(
    alice,           # source node
    'KNOWS',         # relationship type
    bob,             # target node
    {                # properties (optional)
        'since': '2020-01-15',
        'strength': 0.9
    }
)
```

## Next Steps

- Learn about [Nodes](nodes.md)
- Understand [Properties](properties.md)
- Explore [Cypher Queries](../cypher/introduction.md)
