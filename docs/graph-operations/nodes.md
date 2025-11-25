# Working with Nodes

Nodes are the fundamental entities in a graph database. In QilbeeDB, nodes represent entities with properties and labels.

## Node Structure

A node in QilbeeDB consists of:

- **ID** - Unique identifier (automatically assigned)
- **Labels** - One or more labels describing the node type
- **Properties** - Key-value pairs storing node data

```
┌─────────────────────────┐
│  Node ID: 123           │
├─────────────────────────┤
│  Labels: [User, Person] │
├─────────────────────────┤
│  Properties:            │
│    name: "Alice"        │
│    age: 28              │
│    city: "SF"           │
└─────────────────────────┘
```

## Creating Nodes

### Single Label

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_graph")

# Create node with single label
user = graph.create_node(
    ['User'],
    {'name': 'Alice', 'email': 'alice@example.com'}
)

print(f"Created node with ID: {user.id}")
```

### Multiple Labels

```python
# Create node with multiple labels
person = graph.create_node(
    ['Person', 'User', 'Admin'],
    {
        'name': 'Bob',
        'age': 35,
        'role': 'administrator'
    }
)
```

### Cypher CREATE

```python
# Create node using Cypher
result = graph.query("""
    CREATE (u:User {name: $name, email: $email})
    RETURN u
""", {
    "name": "Carol",
    "email": "carol@example.com"
})
```

## Reading Nodes

### Get Node by ID

```python
# Retrieve node by ID
node = graph.get_node(user.id)

print(f"Name: {node.get('name')}")
print(f"Email: {node.get('email')}")
```

### Find Nodes by Label

```python
# Find all nodes with specific label
users = graph.find_nodes('User')

for user in users:
    print(f"User: {user.get('name')}")
```

### Find with Limit

```python
# Limit number of results
recent_users = graph.find_nodes('User', limit=10)
```

### Cypher MATCH

```python
# Query nodes with Cypher
results = graph.query("""
    MATCH (u:User)
    WHERE u.age > $min_age
    RETURN u.name, u.age
    ORDER BY u.age DESC
""", {"min_age": 25})

for row in results:
    print(f"{row['u.name']}: {row['u.age']} years old")
```

## Updating Nodes

### Update Properties

```python
# Update node properties
user.set('age', 29)
user.set('last_login', '2024-11-24')

updated_user = graph.update_node(user)
```

### Cypher SET

```python
# Update using Cypher
graph.query("""
    MATCH (u:User {name: $name})
    SET u.age = $new_age,
        u.updated_at = $timestamp
""", {
    "name": "Alice",
    "new_age": 30,
    "timestamp": "2024-11-24T15:30:00Z"
})
```

### Add/Remove Labels

*Coming soon - Label management operations*

## Deleting Nodes

### Simple Delete

```python
# Delete node (must have no relationships)
graph.delete_node(user.id)
```

### Detach Delete

```python
# Delete node and all its relationships
graph.detach_delete_node(user.id)
```

### Cypher DELETE

```python
# Delete using Cypher
graph.query("""
    MATCH (u:User {name: $name})
    DELETE u
""", {"name": "Alice"})

# Detach delete
graph.query("""
    MATCH (u:User {name: $name})
    DETACH DELETE u
""", {"name": "Bob"})
```

## Node Properties

### Property Types

QilbeeDB supports these property types:

- **String** - Text data
- **Integer** - Whole numbers
- **Float** - Decimal numbers
- **Boolean** - true/false
- **DateTime** - Timestamps
- **Array** - Lists of values (same type)

```python
node = graph.create_node(
    ['Example'],
    {
        'text': 'Hello',
        'number': 42,
        'decimal': 3.14,
        'flag': True,
        'created': '2024-11-24T15:30:00Z',
        'tags': ['python', 'graph', 'database']
    }
)
```

### Access Properties

```python
# Get property
name = node.get('name')

# Get with default
email = node.get('email', 'no-email@example.com')

# Set property
node.set('verified', True)

# Get all properties
props = node.properties
```

## Node Labels

### Multi-Label Nodes

Nodes can have multiple labels for flexible classification:

```python
# User who is also an admin
admin_user = graph.create_node(
    ['User', 'Admin'],
    {'name': 'Alice', 'permissions': 'all'}
)

# Student who is also a teacher
student_teacher = graph.create_node(
    ['Student', 'Teacher'],
    {'name': 'Bob', 'subjects': ['Math', 'Physics']}
)
```

### Query by Multiple Labels

```python
# Find nodes with all specified labels
results = graph.query("""
    MATCH (n:User:Admin)
    RETURN n.name
""")
```

## Best Practices

### 1. Use Meaningful Labels

```python
# Good: Descriptive labels
graph.create_node(['Customer', 'PremiumMember'], {...})

# Bad: Generic labels
graph.create_node(['Entity', 'Thing'], {...})
```

### 2. Keep Properties Simple

```python
# Good: Simple, flat properties
node = graph.create_node(['User'], {
    'name': 'Alice',
    'email': 'alice@example.com',
    'age': 28
})

# Avoid: Complex nested structures
# Use relationships instead
```

### 3. Use Consistent Naming

```python
# Good: Consistent naming convention
graph.create_node(['User'], {
    'user_id': 'U123',
    'first_name': 'Alice',
    'last_name': 'Johnson',
    'created_at': '2024-11-24'
})
```

### 4. Index Frequently Queried Properties

```python
# Create index for fast lookups
# (Index operations coming soon)
```

## Common Patterns

### Upsert (Create or Update)

```python
# Create if not exists, update if exists
result = graph.query("""
    MERGE (u:User {email: $email})
    ON CREATE SET u.name = $name, u.created = timestamp()
    ON MATCH SET u.last_seen = timestamp()
    RETURN u
""", {
    "email": "alice@example.com",
    "name": "Alice"
})
```

### Bulk Creation

```python
# Create multiple nodes efficiently
users_data = [
    {'name': 'Alice', 'age': 28},
    {'name': 'Bob', 'age': 32},
    {'name': 'Carol', 'age': 26}
]

created_nodes = []
for user_data in users_data:
    node = graph.create_node(['User'], user_data)
    created_nodes.append(node)
```

### Conditional Updates

```python
# Update only if condition met
graph.query("""
    MATCH (u:User {name: $name})
    WHERE u.age < $max_age
    SET u.status = 'active'
""", {
    "name": "Alice",
    "max_age": 65
})
```

## Next Steps

- Learn about [Relationships](relationships.md)
- Understand [Properties](properties.md) in detail
- Create [Indexes](indexes.md) for performance
- Use [Transactions](transactions.md) for data integrity

## Related Documentation

- [Cypher CREATE](../cypher/create.md)
- [Cypher MATCH](../cypher/match.md)
- [Python SDK](../client-libraries/python.md)
