# Quick Start Guide

This guide will help you build your first application with QilbeeDB in just a few minutes. We'll create a simple social network graph and perform basic queries.

## Prerequisites

Before starting, ensure you have:

- QilbeeDB server running (see [Installation](installation.md))
- Python 3.8+ installed
- QilbeeDB Python SDK installed: `pip install qilbeedb`

## Step 1: Connect to QilbeeDB

First, let's connect to your QilbeeDB instance:

```python
from qilbeedb import QilbeeDB

# Connect to local QilbeeDB instance
db = QilbeeDB("http://localhost:7474")

# Create or get a graph
graph = db.graph("social_network")
```

## Step 2: Create Your First Nodes

Let's create some user nodes:

```python
# Create Alice
alice = graph.create_node(
    ['User', 'Person'],
    {
        'name': 'Alice Johnson',
        'age': 28,
        'city': 'San Francisco',
        'email': 'alice@example.com'
    }
)

# Create Bob
bob = graph.create_node(
    ['User', 'Person'],
    {
        'name': 'Bob Smith',
        'age': 32,
        'city': 'New York',
        'email': 'bob@example.com'
    }
)

# Create Carol
carol = graph.create_node(
    ['User', 'Person'],
    {
        'name': 'Carol Davis',
        'age': 26,
        'city': 'San Francisco',
        'email': 'carol@example.com'
    }
)

print(f"Created nodes: Alice (ID: {alice.id}), Bob (ID: {bob.id}), Carol (ID: {carol.id})")
```

## Step 3: Create Relationships

Now let's connect these users with friendship relationships:

```python
# Alice knows Bob
alice_bob = graph.create_relationship(
    alice,
    'KNOWS',
    bob,
    {
        'since': '2020-01-15',
        'strength': 0.8
    }
)

# Alice knows Carol
alice_carol = graph.create_relationship(
    alice,
    'KNOWS',
    carol,
    {
        'since': '2021-03-20',
        'strength': 0.9
    }
)

# Bob knows Carol
bob_carol = graph.create_relationship(
    bob,
    'KNOWS',
    carol,
    {
        'since': '2021-06-10',
        'strength': 0.7
    }
)

print("Created friendship relationships")
```

## Step 4: Query the Graph

Let's find Alice's friends using Cypher:

```python
# Find all of Alice's friends
results = graph.query("""
    MATCH (alice:User {name: $name})-[:KNOWS]->(friend)
    RETURN friend.name, friend.city
""", {"name": "Alice Johnson"})

print("Alice's friends:")
for row in results:
    print(f"  - {row['friend.name']} from {row['friend.city']}")
```

## Step 5: Find Common Friends

Let's find if Alice and Bob have any common friends:

```python
# Find common friends between Alice and Bob
results = graph.query("""
    MATCH (alice:User {name: $alice_name})-[:KNOWS]->(common)<-[:KNOWS]-(bob:User {name: $bob_name})
    RETURN common.name, common.email
""", {
    "alice_name": "Alice Johnson",
    "bob_name": "Bob Smith"
})

print("Common friends between Alice and Bob:")
for row in results:
    print(f"  - {row['common.name']} ({row['common.email']})")
```

## Step 6: Update Node Properties

Update a user's information:

```python
# Update Alice's age
alice.set('age', 29)
alice.set('last_updated', '2024-11-24')
updated_alice = graph.update_node(alice)

print(f"Updated Alice's age to {updated_alice.get('age')}")
```

## Step 7: Find Users by Criteria

Query users matching specific criteria:

```python
# Find users in San Francisco over 25
results = graph.query("""
    MATCH (u:User)
    WHERE u.city = $city AND u.age > $min_age
    RETURN u.name, u.age
    ORDER BY u.age DESC
""", {
    "city": "San Francisco",
    "min_age": 25
})

print("Users in San Francisco over 25:")
for row in results:
    print(f"  - {row['u.name']}, age {row['u.age']}")
```

## Complete Example

Here's the complete code for a working social network:

```python
from qilbeedb import QilbeeDB

def main():
    # Connect to QilbeeDB
    db = QilbeeDB("http://localhost:7474")
    graph = db.graph("social_network")

    # Create users
    alice = graph.create_node(
        ['User', 'Person'],
        {'name': 'Alice Johnson', 'age': 28, 'city': 'San Francisco'}
    )

    bob = graph.create_node(
        ['User', 'Person'],
        {'name': 'Bob Smith', 'age': 32, 'city': 'New York'}
    )

    carol = graph.create_node(
        ['User', 'Person'],
        {'name': 'Carol Davis', 'age': 26, 'city': 'San Francisco'}
    )

    # Create friendships
    graph.create_relationship(alice, 'KNOWS', bob, {'since': '2020-01-15'})
    graph.create_relationship(alice, 'KNOWS', carol, {'since': '2021-03-20'})
    graph.create_relationship(bob, 'KNOWS', carol, {'since': '2021-06-10'})

    # Query friends
    results = graph.query("""
        MATCH (alice:User {name: $name})-[:KNOWS]->(friend)
        RETURN friend.name, friend.city
    """, {"name": "Alice Johnson"})

    print("Alice's friends:")
    for row in results:
        print(f"  - {row['friend.name']} from {row['friend.city']}")

if __name__ == "__main__":
    main()
```

## Working with Agent Memory

QilbeeDB's unique feature is native agent memory support. Here's how to use it:

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

# Connect to QilbeeDB
db = QilbeeDB("http://localhost:7474")

# Get agent memory manager
memory = db.agent_memory('customer_service_bot')

# Store conversation episodes
ep1 = Episode.conversation(
    'customer_service_bot',
    'Hi, I need help with my order',
    'Hello! I\'d be happy to help. What\'s your order number?'
)
episode_id = memory.store_episode(ep1)

ep2 = Episode.conversation(
    'customer_service_bot',
    'My order number is #12345',
    'Let me look that up. Your order shipped yesterday and arrives tomorrow.'
)
memory.store_episode(ep2)

# Retrieve recent conversations
recent = memory.get_recent_episodes(10)
print(f"Retrieved {len(recent)} recent episodes")

# Get memory statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance}")
```

## Using the Query Builder

For programmatic query construction:

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
    print(f"{row['f.name']}, age {row['f.age']}")
```

## Error Handling

Always handle errors properly in production:

```python
from qilbeedb import QilbeeDB
from qilbeedb.exceptions import ConnectionError, QueryError, NodeNotFoundError

try:
    db = QilbeeDB("http://localhost:7474")
    graph = db.graph("my_graph")

    # Your operations here
    node = graph.get_node(123)

except ConnectionError as e:
    print(f"Failed to connect to QilbeeDB: {e}")
except QueryError as e:
    print(f"Query failed: {e}")
except NodeNotFoundError as e:
    print(f"Node not found: {e}")
```

## Next Steps

Now that you've built your first QilbeeDB application, explore:

- **[Python SDK Documentation](../client-libraries/python.md)** - Complete SDK reference
- **[Graph Operations](../graph-operations/nodes.md)** - Advanced node and relationship operations
- **[Cypher Queries](../cypher/introduction.md)** - Learn the query language
- **[Agent Memory](../agent-memory/overview.md)** - Deep dive into memory management
- **[Use Cases](../use-cases/ai-agents.md)** - Real-world application examples

## Tips for Success

1. **Use Parameterized Queries**: Always use parameters instead of string interpolation to prevent injection attacks

2. **Use Context Managers**: Always use `with` statements for automatic resource cleanup

3. **Index Your Data**: Create indexes on frequently queried properties for better performance

4. **Batch Operations**: Use transactions for bulk operations

5. **Monitor Performance**: Use the execution statistics to optimize your queries

## Common Patterns

### Creating Multiple Nodes

```python
users = [
    {'name': 'Alice', 'age': 28},
    {'name': 'Bob', 'age': 32},
    {'name': 'Carol', 'age': 26}
]

created_nodes = []
for user in users:
    node = graph.create_node(['User'], user)
    created_nodes.append(node)
```

### Finding Related Nodes

```python
# Find friends of friends
results = graph.query("""
    MATCH (user:User {name: $name})-[:KNOWS*2..2]->(fof)
    WHERE fof.name <> $name
    RETURN DISTINCT fof.name
""", {"name": "Alice Johnson"})
```

### Counting Relationships

```python
# Count how many friends each user has
results = graph.query("""
    MATCH (u:User)-[:KNOWS]->(friend)
    RETURN u.name, COUNT(friend) as friend_count
    ORDER BY friend_count DESC
""")
```

## Troubleshooting

### Connection Issues

If you can't connect to QilbeeDB:

```bash
# Check if server is running
curl http://localhost:7474/health

# Check server logs
docker logs qilbeedb
```

### Query Performance

If queries are slow:

1. Check if you're using indexes
2. Use `EXPLAIN` to see query plan
3. Limit result sets with `LIMIT`
4. Use specific labels in `MATCH` clauses

## Getting Help

- Read the [full documentation](../index.md)
- Check [GitHub issues](https://github.com/aicubetechnology/qilbeeDB/issues)
- Review [example use cases](../use-cases/ai-agents.md)
