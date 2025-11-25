# Graph API

Programmatic API for graph operations.

## Node Operations

### Create Node

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_graph")

# Create node
node = graph.create_node(
    labels=['User', 'Person'],
    properties={'name': 'Alice', 'age': 28}
)
```

### Read Node

```python
# Get node by ID
node = graph.get_node(node_id=123)

# Get nodes by label
users = graph.get_nodes_by_label('User', limit=10)
```

### Update Node

```python
# Update properties
graph.update_node(
    node_id=123,
    properties={'age': 29}
)
```

### Delete Node

```python
# Delete node and relationships
graph.delete_node(node_id=123, detach=True)
```

## Relationship Operations

### Create Relationship

```python
# Create relationship
rel = graph.create_relationship(
    start_node=123,
    rel_type='KNOWS',
    end_node=456,
    properties={'since': '2023-01-15'}
)
```

### Query Relationships

```python
# Get relationships for node
rels = graph.get_relationships(node_id=123, direction='outgoing')
```

### Delete Relationship

```python
graph.delete_relationship(rel_id=789)
```

## Cypher Queries

```python
# Execute Cypher query
results = graph.query("""
    MATCH (u:User)-[:KNOWS]->(f:User)
    WHERE u.name = $name
    RETURN f.name, f.age
""", parameters={'name': 'Alice'})

for row in results:
    print(f"{row['f.name']}, {row['f.age']}")
```

## Query Builder

```python
from qilbeedb.query import QueryBuilder

# Build query programmatically
query = (QueryBuilder()
    .match("(u:User)")
    .where("u.age > $min_age")
    .return_("u.name", "u.age")
    .order_by("u.age", desc=True)
    .limit(10)
)

results = graph.execute(query, {'min_age': 25})
```

## Transactions

```python
# Use transaction
with graph.transaction() as tx:
    alice = tx.create_node(['User'], {'name': 'Alice'})
    bob = tx.create_node(['User'], {'name': 'Bob'})
    tx.create_relationship(alice, 'KNOWS', bob)
    # Automatically committed
```

## Batch Operations

```python
# Batch create nodes
nodes = [
    {'labels': ['User'], 'properties': {'name': f'User{i}'}}
    for i in range(1000)
]
graph.batch_create_nodes(nodes)
```

## Next Steps

- Learn about [HTTP API](http-api.md)
- Explore [Memory API](memory-api.md)
- Use the [Python SDK](../client-libraries/python.md)
