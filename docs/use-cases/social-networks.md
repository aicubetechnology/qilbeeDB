# Social Networks

Graph databases excel at social network applications where relationships between users are first-class entities. QilbeeDB provides high-performance graph operations perfect for social platforms.

## Core Use Cases

### Friend Networks

Model friendships and social connections:

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
graph = db.graph("social_network")

# Create users
alice = graph.create_node(['User'], {
    'username': 'alice',
    'name': 'Alice Johnson',
    'joined': '2023-01-15'
})

bob = graph.create_node(['User'], {
    'username': 'bob', 
    'name': 'Bob Smith',
    'joined': '2023-02-20'
})

# Create friendship
graph.create_relationship(alice, 'FRIEND', bob, {
    'since': '2023-03-01',
    'strength': 0.9
})
```

### Find Friends of Friends

```python
# Discover new connections
results = graph.query("""
    MATCH (user:User {username: $username})-[:FRIEND]->(friend)-[:FRIEND]->(fof)
    WHERE fof.username <> $username
      AND NOT (user)-[:FRIEND]->(fof)
    RETURN DISTINCT fof.name, fof.username
    LIMIT 10
""", {"username": "alice"})
```

### Influencer Detection

Find highly connected users:

```python
results = graph.query("""
    MATCH (u:User)-[:FRIEND]-(friend)
    RETURN u.username, u.name, COUNT(friend) as friend_count
    ORDER BY friend_count DESC
    LIMIT 20
""")
```

### Community Detection

Identify clusters of users:

```python
# Find dense subgraphs
results = graph.query("""
    MATCH (u1:User)-[:FRIEND]-(u2:User),
          (u2)-[:FRIEND]-(u3:User),
          (u3)-[:FRIEND]-(u1)
    RETURN u1.username, u2.username, u3.username
""")
```

## Next Steps

- Learn about [Graph Operations](../graph-operations/nodes.md)
- Explore [Cypher Queries](../cypher/introduction.md)
- Read the [Python SDK](../client-libraries/python.md)
