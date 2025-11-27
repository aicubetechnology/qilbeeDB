# Python SDK

The QilbeeDB Python SDK provides a complete interface for interacting with QilbeeDB, including graph operations, Cypher queries, and agent memory management.

## Installation

```bash
# Install from PyPI (coming soon)
pip install qilbeedb

# Or install from source
cd sdks/python
pip install -e .
```

## Basic Usage

### Connecting to QilbeeDB

```python
from qilbeedb import QilbeeDB

# Connect to QilbeeDB
db = QilbeeDB("http://localhost:7474")

# Or use with context manager
with QilbeeDB("http://localhost:7474") as db:
    graph = db.graph("my_graph")
    # Use the graph...
```

### Creating Graphs

```python
# Get or create a graph
graph = db.graph("my_graph")

# List all graphs
graphs = db.list_graphs()
```

## Working with Nodes

### Creating Nodes

```python
# Create node with single label
user = graph.create_node(['User'], {
    'name': 'Alice',
    'email': 'alice@example.com'
})

# Create node with multiple labels
person = graph.create_node(
    ['Person', 'User', 'Admin'],
    {
        'name': 'Bob',
        'age': 35,
        'city': 'San Francisco'
    }
)
```

### Reading Nodes

```python
# Get node by ID
node = graph.get_node(user.id)

# Find nodes by label
users = graph.find_nodes('User')

# Find nodes with limit
recent_users = graph.find_nodes('User', limit=10)
```

### Updating Nodes

```python
# Update node properties
user.set('age', 31)
user.set('updated_at', '2024-01-15')
graph.update_node(user)
```

### Deleting Nodes

```python
# Delete node (must have no relationships)
graph.delete_node(user.id)

# Detach delete (removes relationships too)
graph.detach_delete_node(user.id)
```

## Working with Relationships

### Creating Relationships

```python
# Create relationship between nodes
friendship = graph.create_relationship(
    alice,           # source node or ID
    'KNOWS',         # relationship type
    bob,             # target node or ID
    {                # properties (optional)
        'since': '2020-01-15',
        'strength': 0.9
    }
)
```

### Reading Relationships

```python
# Get all relationships for a node
relationships = graph.get_relationships(alice)

# Get outgoing relationships only
outgoing = graph.get_relationships(alice, direction='outgoing')

# Get incoming relationships only
incoming = graph.get_relationships(bob, direction='incoming')
```

## Cypher Queries

### Basic Queries

```python
# Simple query
results = graph.query("MATCH (n:Person) RETURN n.name, n.age")

for row in results:
    print(f"Name: {row['n.name']}, Age: {row['n.age']}")
```

### Parameterized Queries

```python
# Query with parameters (recommended for security)
results = graph.query("""
    MATCH (p:Person)
    WHERE p.age > $min_age AND p.city = $city
    RETURN p.name, p.age
    ORDER BY p.age DESC
    LIMIT $limit
""", {
    "min_age": 25,
    "city": "San Francisco",
    "limit": 10
})
```

### Complex Queries

```python
# Relationship traversal
results = graph.query("""
    MATCH (user:User {name: $username})-[:KNOWS]->(friend)-[:KNOWS]->(fof)
    WHERE fof.name <> $username
    RETURN DISTINCT fof.name, fof.email
    LIMIT 20
""", {"username": "Alice"})
```

## Query Builder

For programmatic query construction, use the fluent Query Builder API:

```python
from qilbeedb.query import Query

# Build query fluently
results = (
    Query(graph)
    .match('(p:Person)-[:KNOWS]->(f:Person)')
    .where('p.city = $city', {'city': 'San Francisco'})
    .return_clause('f.name', 'f.age')
    .order_by('f.age', desc=True)
    .limit(10)
    .execute()
)

for row in results:
    print(row)
```

## Agent Memory

### Storing Episodes

```python
from qilbeedb.memory import Episode

# Get agent memory manager
memory = db.agent_memory('customer_service_bot')

# Store conversation episode
episode = Episode.conversation(
    agent_id='customer_service_bot',
    user_input='How do I reset my password?',
    agent_response='You can reset your password by clicking...'
)
episode_id = memory.store_episode(episode)

# Store observation
observation = Episode.observation(
    agent_id='customer_service_bot',
    observation='User seems frustrated with login process'
)
memory.store_episode(observation)

# Store action
action = Episode.action(
    agent_id='customer_service_bot',
    action='Sent password reset email',
    result='Email sent successfully'
)
memory.store_episode(action)
```

### Retrieving Episodes

```python
# Get recent episodes
recent = memory.get_recent_episodes(10)
for episode in recent:
    print(f"Type: {episode.episode_type}")
    print(f"Content: {episode.content}")
    print(f"Time: {episode.event_time}")
```

### Memory Statistics

```python
# Get memory statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance}")
print(f"Oldest episode: {stats.oldest_episode}")
print(f"Newest episode: {stats.newest_episode}")
```

## Real-World Examples

### Social Network

```python
# Create social network
db = QilbeeDB("http://localhost:7474")
graph = db.graph("social_network")

# Create users
alice = graph.create_node(['User', 'Person'], {
    'username': 'alice',
    'name': 'Alice Johnson',
    'age': 28,
    'city': 'San Francisco'
})

bob = graph.create_node(['User', 'Person'], {
    'username': 'bob',
    'name': 'Bob Smith',
    'age': 32,
    'city': 'New York'
})

# Create friendship
friendship = graph.create_relationship(
    alice, 'FRIEND', bob,
    {'since': '2023-02-25', 'strength': 0.8}
)

# Find friends
results = graph.query("""
    MATCH (user:User {username: $username})-[:FRIEND]->(friend)
    RETURN friend.name, friend.city
""", {"username": "alice"})
```

### Knowledge Graph

```python
# Create knowledge graph
graph = db.graph("knowledge_base")

# Create concepts
python = graph.create_node(['Concept', 'ProgrammingLanguage'], {
    'name': 'Python',
    'paradigm': 'multi-paradigm',
    'year': 1991
})

web_dev = graph.create_node(['Concept', 'Domain'], {
    'name': 'Web Development',
    'category': 'software'
})

# Create semantic relationship
graph.create_relationship(
    python, 'USED_FOR', web_dev,
    {'popularity': 0.9}
)

# Query concepts
results = graph.query("""
    MATCH (lang:ProgrammingLanguage)-[:USED_FOR]->(domain:Domain)
    WHERE domain.name = $domain_name
    RETURN lang.name, lang.paradigm
""", {"domain_name": "Web Development"})
```

### Recommendation System

```python
# Create recommendation graph
graph = db.graph("recommendations")

# Create user and products
user = graph.create_node(['Customer'], {
    'user_id': 'U001',
    'name': 'Jane Doe'
})

laptop = graph.create_node(['Product'], {
    'product_id': 'P001',
    'name': 'Laptop Pro',
    'category': 'Electronics'
})

# Track purchase
graph.create_relationship(
    user, 'PURCHASED', laptop,
    {'date': '2024-01-15', 'rating': 5}
)

# Find recommendations
recommendations = graph.query("""
    MATCH (u:Customer {user_id: $user_id})-[:PURCHASED]->(p:Product)
          <-[:PURCHASED]-(similar:Customer)-[:PURCHASED]->(rec:Product)
    WHERE NOT (u)-[:PURCHASED]->(rec)
    RETURN rec.name, COUNT(similar) as score
    ORDER BY score DESC
    LIMIT 5
""", {"user_id": "U001"})
```

## Audit Logging

Query and monitor security events (requires Admin role):

```python
from qilbeedb import QilbeeDB

# Login as admin
db = QilbeeDB("http://localhost:7474")
db.login("admin", "Admin123!@#")

# Query all audit logs
result = db.get_audit_logs(limit=100)
print(f"Total events: {result['count']}")
for event in result['events']:
    print(f"{event['event_time']}: {event['event_type']} - {event['result']}")
```

### Filtering Audit Logs

```python
# Filter by event type
login_events = db.get_audit_logs(event_type="login", limit=50)

# Filter by username
user_events = db.get_audit_logs(username="admin", limit=50)

# Filter by result
failed_events = db.get_audit_logs(result="unauthorized", limit=50)

# Filter by time range
recent_events = db.get_audit_logs(
    start_time="2025-01-01T00:00:00Z",
    end_time="2025-12-31T23:59:59Z",
    limit=100
)
```

### Convenience Methods

```python
# Get recent failed login attempts
failed_logins = db.get_failed_logins(limit=20)
for event in failed_logins:
    print(f"Failed login from {event['ip_address']} at {event['event_time']}")

# Get all events for a specific user
user_activity = db.get_user_audit_events("alice", limit=50)

# Get security-relevant events (unauthorized/forbidden)
security_events = db.get_security_events(limit=50)
```

### Audit Event Types

| Category | Event Types |
|----------|-------------|
| Authentication | `login`, `logout`, `login_failed`, `token_refresh`, `token_refresh_failed` |
| User Management | `user_created`, `user_updated`, `user_deleted`, `password_changed` |
| Role Management | `role_assigned`, `role_removed` |
| API Keys | `api_key_created`, `api_key_revoked`, `api_key_used`, `api_key_validation_failed` |
| Authorization | `permission_denied`, `access_granted` |
| Rate Limiting | `rate_limit_exceeded` |

## Error Handling

```python
from qilbeedb.exceptions import (
    ConnectionError,
    QueryError,
    NodeNotFoundError,
    AuthenticationError
)

try:
    db = QilbeeDB("http://localhost:7474")
    db.login("admin", "password")
    graph = db.graph("my_graph")

    # Your operations here
    node = graph.get_node(123)

except AuthenticationError as e:
    print(f"Authentication failed: {e}")
except ConnectionError as e:
    print(f"Failed to connect: {e}")
except QueryError as e:
    print(f"Query failed: {e}")
except NodeNotFoundError as e:
    print(f"Node not found: {e}")
```

## Best Practices

### Use Parameterized Queries

Always use parameters instead of string interpolation:

```python
# ✅ GOOD: Parameterized query
results = graph.query(
    "MATCH (p:Person) WHERE p.name = $name RETURN p",
    {"name": user_input}
)

# ❌ BAD: String interpolation (vulnerable to injection)
results = graph.query(
    f"MATCH (p:Person) WHERE p.name = '{user_input}' RETURN p"
)
```

### Use Context Managers

Always use context managers for automatic cleanup:

```python
# ✅ GOOD: Context manager
with QilbeeDB("http://localhost:7474") as db:
    graph = db.graph("my_graph")
    # Operations...

# ❌ BAD: Manual management
db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_graph")
# Operations...
db.close()  # Easy to forget!
```

### Batch Operations

Use transactions for batch operations:

```python
# Create multiple nodes efficiently
with db.transaction() as tx:
    for i in range(1000):
        tx.create_node(['User'], {'user_id': f'U{i:04d}'})
    tx.commit()
```

## Next Steps

- Learn about [Graph Operations](../graph-operations/nodes.md)
- Explore [Cypher Queries](../cypher/introduction.md)
- Understand [Agent Memory](../agent-memory/overview.md)
- Check out [Use Cases](../use-cases/ai-agents.md)
