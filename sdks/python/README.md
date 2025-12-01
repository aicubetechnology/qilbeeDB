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

QilbeeDB provides enterprise-grade agent memory with automatic persistence. All episodes are stored in RocksDB with write-ahead logging, ensuring durability across server restarts.

```python
from qilbeedb.memory import Episode

# Create agent memory
memory = db.agent_memory(
    "agent-001",
    max_episodes=10000,
    min_relevance=0.1
)

# Store conversation (automatically persisted to disk)
episode = Episode.conversation(
    "agent-001",
    "What is the capital of France?",
    "The capital of France is Paris."
)
episode_id = memory.store_episode(episode)
print(f"Stored episode: {episode_id}")  # Episode survives server restart

# Store observation
obs = Episode.observation(
    "agent-001",
    "User seems interested in European geography"
)
memory.store_episode(obs)

# Store action with result
action = Episode.action(
    "agent-001",
    "Searched knowledge base for European capitals",
    "Found 47 capital cities"
)
memory.store_episode(action)

# Retrieve recent memories
recent = memory.get_recent_episodes(10)
for ep in recent:
    print(f"[{ep.episode_type}] {ep.content}")

# Get a specific episode by ID
retrieved = memory.get_episode(episode_id)

# Search memories
results = memory.search_episodes("France")

# Delete a specific episode
memory.delete_episode(episode_id)

# Get statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance:.2f}")

# Consolidate and forget
memory.consolidate()
memory.forget(min_relevance=0.2)

# Clear all memories for an agent
memory.clear()
```

### Semantic Search

Find memories by meaning, not just keywords. Semantic search uses vector embeddings to find conceptually similar content.

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
memory = db.agent_memory("my-agent")

# Store some episodes
memory.store_episode(Episode.conversation(
    "my-agent",
    "What is machine learning?",
    "ML is a subset of artificial intelligence..."
))
memory.store_episode(Episode.conversation(
    "my-agent",
    "How do neural networks work?",
    "Neural networks are computing systems inspired by the brain..."
))

# Semantic search - finds related content even with different words
results = memory.semantic_search("AI training techniques", limit=5)
for result in results:
    print(f"Score: {result.score:.2f}")
    print(f"Episode: {result.episode.content}")

# Hybrid search - combines keyword and semantic matching
results = memory.hybrid_search(
    "machine learning",
    limit=10,
    semantic_weight=0.5  # 0.0 = keyword only, 1.0 = semantic only
)
for result in results:
    print(f"Combined: {result.score:.2f}")
    print(f"Keyword: {result.keyword_score}, Semantic: {result.semantic_score}")

# Find episodes similar to a specific episode
similar = memory.find_similar_episodes(episode_id, limit=5)
for result in similar:
    print(f"Similar episode (score {result.score:.2f}): {result.episode.id}")

# Check semantic search configuration
status = memory.get_semantic_search_status()
if status["enabled"]:
    print(f"Model: {status['model']}")
    print(f"Indexed: {status['indexedEpisodes']} episodes")
```

For more information, see the [Semantic Search documentation](https://docs.qilbeedb.com/agent-memory/semantic-search/).

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
- `get_audit_logs(**filters) -> Dict` - Query audit logs (admin only)
- `get_failed_logins(limit: int) -> List` - Get failed login events
- `get_user_audit_events(username: str, limit: int) -> List` - Get events for a user
- `get_security_events(limit: int) -> List` - Get security-related events
- `get_locked_accounts() -> Dict` - Get all locked accounts (admin only)
- `get_lockout_status(username: str) -> Dict` - Get lockout status for a user (admin only)
- `lock_account(username: str, reason: str) -> Dict` - Manually lock an account (admin only)
- `unlock_account(username: str) -> Dict` - Unlock an account (admin only)

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

Bi-temporal agent memory with automatic RocksDB persistence.

All episodes are automatically persisted to disk with write-ahead logging (WAL),
ensuring durability across server restarts.

**Methods:**
- `store_episode(episode: Episode) -> str` - Store episode (persisted to disk)
- `get_episode(episode_id: str) -> Episode` - Get episode by ID
- `get_recent_episodes(limit: int) -> List[Episode]` - Get recent episodes
- `search_episodes(query: str, limit: int) -> List[Episode]` - Search episodes
- `semantic_search(query: str, limit: int, min_score: float) -> List[SemanticSearchResult]` - Search by meaning
- `hybrid_search(query: str, limit: int, semantic_weight: float) -> List[HybridSearchResult]` - Combined search
- `find_similar_episodes(episode_id: str, limit: int) -> List[SemanticSearchResult]` - Find similar episodes
- `get_semantic_search_status() -> Dict` - Get semantic search configuration status
- `delete_episode(episode_id: str) -> bool` - Delete episode by ID
- `get_statistics() -> MemoryStatistics` - Get statistics
- `consolidate() -> int` - Consolidate memory
- `forget(min_relevance: float) -> int` - Forget low-relevance episodes
- `clear() -> bool` - Clear all episodes for this agent

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

## Memory Persistence

QilbeeDB provides enterprise-grade memory persistence using RocksDB as the storage backend. All agent memories are automatically persisted to disk, ensuring durability across server restarts.

### Key Features

- **Automatic Durability**: Episodes are automatically persisted when stored - no explicit save required
- **Write-Ahead Logging (WAL)**: Ensures crash recovery and transaction safety
- **LZ4 Compression**: Reduces storage footprint with minimal overhead
- **Agent Isolation**: Episodes are stored in separate namespaces per agent

### Verifying Persistence

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

# Store episode
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
memory = db.agent_memory("my-agent")

episode = Episode.conversation(
    "my-agent",
    "What is QilbeeDB?",
    "QilbeeDB is a graph database with agent memory..."
)
episode_id = memory.store_episode(episode)
print(f"Stored episode: {episode_id}")

# After server restart, episode is still available
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")

# Retrieve the persisted episode
retrieved = memory.get_episode(episode_id)
print(f"Retrieved: {retrieved.content}")
```

For more information, see the [Memory Persistence documentation](https://docs.qilbeedb.com/agent-memory/persistence/).

## Audit Logging (Admin Only)

Query and monitor security events for compliance and debugging:

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

# Convenience methods
failed_logins = db.get_failed_logins(limit=20)
user_activity = db.get_user_audit_events("admin", limit=50)
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
| Account Lockout | `account_lockout_triggered`, `account_locked`, `account_unlocked` |

## Account Lockout Management (Admin Only)

Monitor and manage locked accounts due to failed login attempts:

```python
from qilbeedb import QilbeeDB

# Login as admin
db = QilbeeDB("http://localhost:7474")
db.login("admin", "Admin123!@#")

# Get all locked accounts
locked = db.get_locked_accounts()
print(f"Total locked accounts: {locked['count']}")
for user, status in locked['locked_users']:
    print(f"  {user}: {status['lockout_remaining_seconds']}s remaining")

# Get lockout status for a specific user
status = db.get_lockout_status("suspicious_user")
print(f"User: {status['username']}")
print(f"  Locked: {status['status']['locked']}")
print(f"  Failed attempts: {status['status']['failed_attempts']}")
print(f"  Remaining attempts: {status['status']['remaining_attempts']}")
print(f"  Lockout count: {status['status']['lockout_count']}")

# Manually lock an account (e.g., suspicious activity)
result = db.lock_account("suspicious_user", reason="Suspicious activity detected")
print(f"Lock result: {result['success']}")

# Unlock an account
result = db.unlock_account("suspicious_user")
print(f"Unlock result: {result['success']}")
```

### Lockout Behavior

- **Default max attempts:** 5 failed logins
- **Initial lockout duration:** 15 minutes
- **Progressive lockout:** Duration doubles with each subsequent lockout (up to 24 hours)
- **Automatic unlock:** Accounts unlock automatically after lockout period expires

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
