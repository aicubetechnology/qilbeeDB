# QilbeeDB Python SDK

Official Python client library for QilbeeDB - Enterprise Graph Database with Bi-Temporal Agent Memory.

## Installation

```bash
pip install qilbeedb
```

## Quick Start

### Authentication

QilbeeDB supports two authentication methods:

#### JWT Authentication (for humans/admins)
```python
from qilbeedb import QilbeeDB

# Connect and login with username/password
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
```

#### API Key Authentication (recommended for applications)
```python
# Option 1: Initialize with API key
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": "qilbee_live_your_api_key_here"
})

# Option 2: Switch to API key after JWT login
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
db.set_api_key("qilbee_live_your_api_key_here")
```

### Basic Graph Operations

```python
from qilbeedb import QilbeeDB

# Connect to database
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")

# Get or create a graph
graph = db.graph("social")

# Create nodes
alice = graph.create_node(
    labels=["Person"],
    properties={"name": "Alice", "age": 30}
)

bob = graph.create_node(
    labels=["Person"],
    properties={"name": "Bob", "age": 35}
)

# Create relationship
knows = graph.create_relationship(
    alice.id,
    "KNOWS",
    bob.id,
    properties={"since": 2020}
)

# Query nodes
people = graph.find_nodes("Person")
for person in people:
    print(f"{person['name']} is {person['age']} years old")
```

### Cypher Queries

```python
# Execute Cypher query
result = graph.query(
    "MATCH (p:Person) WHERE p.age > $age RETURN p.name, p.age ORDER BY p.age DESC",
    parameters={"age": 25}
)

for record in result:
    print(f"{record['p.name']}: {record['p.age']}")

# Query statistics
print(f"Execution time: {result.stats.execution_time_ms}ms")
```

### Query Builder

```python
from qilbeedb.query import Query

# Build query fluently
result = (
    Query(graph)
    .match("(p:Person)")
    .where("p.age > $age", age=25)
    .return_clause("p.name", "p.age")
    .order_by("p.age", desc=True)
    .limit(10)
    .execute()
)
```

### Agent Memory

```python
from qilbeedb.memory import Episode

# Create agent memory
memory = db.agent_memory(
    "agent-001",
    max_episodes=10000,
    min_relevance=0.1
)

# Store conversation
episode = Episode.conversation(
    "agent-001",
    "What is the capital of France?",
    "The capital of France is Paris."
)
memory.store_episode(episode)

# Store observation
obs = Episode.observation(
    "agent-001",
    "User seems interested in European geography"
)
memory.store_episode(obs)

# Retrieve recent memories
recent = memory.get_recent_episodes(10)
for ep in recent:
    print(ep.content)

# Search memories
results = memory.search_episodes("France")

# Get statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance:.2f}")

# Consolidate and forget
memory.consolidate()
memory.forget(min_relevance=0.2)
```

### Context Manager

```python
with QilbeeDB("http://localhost:7474") as db:
    graph = db.graph("mydata")
    # Your code here
# Connection automatically closed
```

## API Reference

### QilbeeDB

Main database client.

**Methods:**
- `graph(name: str) -> Graph` - Get or create graph
- `list_graphs() -> List[str]` - List all graphs
- `delete_graph(name: str) -> bool` - Delete graph
- `health() -> Dict` - Get health status
- `agent_memory(agent_id: str, **config) -> AgentMemory` - Create agent memory
- `login(username: str, password: str) -> Dict` - Login with JWT authentication
- `logout() -> None` - Logout and clear authentication
- `is_authenticated() -> bool` - Check if authenticated
- `set_api_key(api_key: str) -> None` - Switch to API key authentication
- `refresh_token() -> str` - Manually refresh JWT access token

### Graph

Graph operations.

**Methods:**
- `create_node(labels: List[str], properties: Dict) -> Node` - Create node
- `get_node(node_id: int) -> Node` - Get node by ID
- `update_node(node: Node) -> Node` - Update node
- `delete_node(node_id: int) -> bool` - Delete node
- `create_relationship(from_node, rel_type: str, to_node, properties: Dict) -> Relationship` - Create relationship
- `find_nodes(label: str, properties: Dict, limit: int) -> List[Node]` - Find nodes
- `get_relationships(node, direction: str) -> List[Relationship]` - Get relationships
- `query(cypher: str, parameters: Dict) -> QueryResult` - Execute Cypher query

### Node

Graph node.

**Attributes:**
- `id: int` - Node ID
- `labels: List[str]` - Node labels
- `properties: Dict` - Node properties

**Methods:**
- `get(key, default) -> Any` - Get property
- `__getitem__(key) -> Any` - Access property: `node["name"]`
- `__setitem__(key, value)` - Set property: `node["age"] = 31`

### Relationship

Graph relationship.

**Attributes:**
- `id: int` - Relationship ID
- `type: str` - Relationship type
- `start_node: int` - Start node ID
- `end_node: int` - End node ID
- `properties: Dict` - Relationship properties

### AgentMemory

Bi-temporal agent memory.

**Methods:**
- `store_episode(episode: Episode) -> str` - Store episode
- `get_episode(episode_id: str) -> Episode` - Get episode
- `get_recent_episodes(limit: int) -> List[Episode]` - Get recent episodes
- `search_episodes(query: str, limit: int) -> List[Episode]` - Search episodes
- `get_statistics() -> MemoryStatistics` - Get statistics
- `consolidate() -> int` - Consolidate memory
- `forget(min_relevance: float) -> int` - Forget low-relevance episodes
- `clear() -> bool` - Clear all episodes

### Episode

Episodic memory.

**Static Methods:**
- `Episode.conversation(agent_id, user_input, agent_response)` - Create conversation
- `Episode.observation(agent_id, observation)` - Create observation
- `Episode.action(agent_id, action, result)` - Create action

## Configuration

### Connection Options

```python
# Simple URI connection (use login() afterward)
db = QilbeeDB("http://localhost:7474")

# Configuration dict with API key (recommended for applications)
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": "qilbee_live_your_api_key_here",
    "timeout": 30,
    "verify_ssl": True
})

# Configuration dict with basic auth (deprecated, use login() instead)
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "auth": {"username": "admin", "password": "password"},
    "timeout": 30,
    "verify_ssl": True,
    "persist_tokens": True
})
```

### Managing API Keys

Create and manage API keys for application authentication:

```python
import requests

# Login as admin to manage API keys
db = QilbeeDB("http://localhost:7474")
db.login("admin", "Admin123!@#")

# Create a new API key
response = db.session.post(
    "http://localhost:7474/api/v1/api-keys",
    json={"name": "my-app-key"}
)
api_key_data = response.json()
api_key = api_key_data["key"]
key_id = api_key_data["id"]

print(f"Created API key: {api_key}")

# List all API keys
response = db.session.get("http://localhost:7474/api/v1/api-keys")
api_keys = response.json()["api_keys"]

# Delete an API key
db.session.delete(f"http://localhost:7474/api/v1/api-keys/{key_id}")

# Now use the API key in your application
app_db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": api_key
})
```

### Memory Configuration

```python
memory = db.agent_memory(
    "agent-001",
    max_episodes=10000,
    min_relevance=0.1,
    auto_consolidate=True,
    auto_forget=True,
    consolidation_threshold=5000,
    episodic_retention_days=30
)
```

## Error Handling

```python
from qilbeedb.exceptions import (
    QilbeeDBError,
    ConnectionError,
    QueryError,
    AuthenticationError
)

try:
    result = graph.query("INVALID QUERY")
except QueryError as e:
    print(f"Query failed: {e}")
except ConnectionError as e:
    print(f"Connection failed: {e}")
except QilbeeDBError as e:
    print(f"Database error: {e}")
```

## Development

### Running Tests

```bash
pip install -e .[dev]
pytest tests/
```

### Code Formatting

```bash
black qilbeedb/
flake8 qilbeedb/
mypy qilbeedb/
```

## License

Apache License 2.0

## Support

- Documentation: https://docs.qilbeedb.com
- Issues: https://github.com/your-org/qilbeedb/issues
- Email: contact@aicube.ca
