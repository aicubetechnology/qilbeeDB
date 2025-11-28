# Python SDK

The QilbeeDB Python SDK provides a complete interface for interacting with QilbeeDB, including graph operations, Cypher queries, agent memory management, and comprehensive security features.

[![PyPI version](https://badge.fury.io/py/qilbeedb.svg)](https://pypi.org/project/qilbeedb/)
[![Python 3.8+](https://img.shields.io/badge/python-3.8+-blue.svg)](https://www.python.org/downloads/)

## Installation

```bash
# Install from PyPI
pip install qilbeedb

# Or install from source
cd sdks/python
pip install -e .
```

## Quick Start

```python
from qilbeedb import QilbeeDB

# Connect and authenticate
db = QilbeeDB("http://localhost:7474")
db.login("admin", "SecureAdmin@123!")

# Create a graph and add nodes
graph = db.graph("my_graph")
alice = graph.create_node(["Person"], {"name": "Alice", "age": 30})
bob = graph.create_node(["Person"], {"name": "Bob", "age": 35})

# Create a relationship
graph.create_relationship(alice.id, "KNOWS", bob.id, {"since": 2020})

# Query with Cypher
results = graph.query("MATCH (p:Person) RETURN p.name, p.age")
for row in results:
    print(f"{row['p.name']}: {row['p.age']}")
```

## Authentication

QilbeeDB supports multiple authentication methods for different use cases.

### JWT Authentication (Recommended for Users)

```python
from qilbeedb import QilbeeDB

# Connect and login with username/password
db = QilbeeDB("http://localhost:7474")
db.login("admin", "SecureAdmin@123!")

# Check authentication status
if db.is_authenticated():
    print("Successfully authenticated")

# Logout when done
db.logout()
```

### API Key Authentication (Recommended for Applications)

API keys are recommended for automated applications, CI/CD pipelines, and service-to-service communication.

```python
# Option 1: Initialize with API key directly
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": "qilbee_live_your_api_key_here"
})

# Option 2: Switch to API key after JWT login
db = QilbeeDB("http://localhost:7474")
db.login("admin", "SecureAdmin@123!")
db.set_api_key("qilbee_live_your_api_key_here")
```

### Connection Options

```python
# Full configuration options
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": "qilbee_live_your_api_key_here",  # or use login()
    "timeout": 30,          # Request timeout in seconds
    "verify_ssl": True,     # Verify SSL certificates
    "persist_tokens": True  # Persist JWT tokens
})
```

### Context Manager

```python
# Automatic cleanup with context manager
with QilbeeDB("http://localhost:7474") as db:
    db.login("admin", "SecureAdmin@123!")
    graph = db.graph("my_graph")
    # Operations...
# Connection automatically closed
```

## Working with Graphs

### Creating and Managing Graphs

```python
# Get or create a graph
graph = db.graph("my_graph")

# List all graphs
graphs = db.list_graphs()
print(f"Available graphs: {graphs}")

# Create a new graph explicitly
new_graph = db.create_graph("analytics_graph")

# Delete a graph
db.delete_graph("old_graph")
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

## User Management (Admin Only)

Administrators can create, update, and manage user accounts.

### Creating Users

```python
# Create a new user
user = db.create_user(
    username="developer1",
    password="SecureP@ss123!",
    email="dev1@example.com",
    roles=["Developer", "Read"]
)
print(f"Created user: {user['id']}")
```

### Available Roles

| Role | Description |
|------|-------------|
| `Admin` | Full administrative access |
| `Developer` | Development and testing access |
| `DataScientist` | Analytics and query access |
| `Agent` | AI agent operations |
| `Read` | Read-only access |

### Listing and Managing Users

```python
# List all users
users = db.list_users()
for user in users:
    print(f"{user['username']}: {user['roles']}")

# Get a specific user
user = db.get_user("user-uuid-here")

# Update user roles
db.update_user_roles("user-uuid-here", ["Developer", "DataScientist"])

# Update user password
db.update_user("user-uuid-here", password="NewSecureP@ss!")

# Delete a user
db.delete_user("user-uuid-here")
```

## API Key Management

Create and manage API keys for application authentication.

### Creating API Keys

```python
# Create a new API key
result = db.create_api_key("my-application-key")
api_key = result["key"]  # Store this securely!
key_id = result["id"]
print(f"Created API key: {api_key[:20]}...")
```

!!! warning "Security Note"
    The API key is only shown once when created. Store it securely as it cannot be retrieved later.

### Managing API Keys

```python
# List all API keys (key values are masked)
keys = db.list_api_keys()
for key in keys:
    print(f"ID: {key['id']}, Name: {key['name']}, Created: {key['created_at']}")

# Delete an API key
db.delete_api_key("key-uuid-here")
```

### Using API Keys

```python
# Use the API key in your application
app_db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": "qilbee_live_your_api_key_here"
})

# All operations use the API key automatically
graph = app_db.graph("my_graph")
```

## Rate Limit Policy Management (Admin Only)

Administrators can create custom rate limiting policies for different endpoints.

### Creating Rate Limit Policies

```python
# Create a custom rate limit policy
policy = db.create_rate_limit_policy(
    name="API Strict Limit",
    endpoint_type="GeneralApi",  # or {"Custom": "/api/special"}
    max_requests=100,
    window_secs=3600,  # 100 requests per hour
    enabled=True
)
print(f"Created policy: {policy['id']}")
```

### Endpoint Types

| Type | Description |
|------|-------------|
| `Login` | Authentication endpoints |
| `ApiKeyCreation` | API key creation endpoints |
| `GeneralApi` | General API endpoints |
| `UserManagement` | User management endpoints |
| `{"Custom": "/path"}` | Custom endpoint pattern |

### Managing Policies

```python
# List all rate limit policies
policies = db.list_rate_limit_policies()
for policy in policies:
    print(f"{policy['name']}: {policy['max_requests']} req/{policy['window_secs']}s")

# Get a specific policy
policy = db.get_rate_limit_policy("policy-uuid-here")

# Update a policy
db.update_rate_limit_policy(
    policy_id="policy-uuid-here",
    max_requests=200,
    enabled=False
)

# Delete a policy
db.delete_rate_limit_policy("policy-uuid-here")
```

## Account Lockout Management (Admin Only)

Monitor and manage locked accounts due to failed login attempts.

### Viewing Locked Accounts

```python
# Get all locked accounts
locked = db.get_locked_accounts()
print(f"Total locked accounts: {locked['count']}")
for user, status in locked['locked_users']:
    print(f"  {user}: {status['lockout_remaining_seconds']}s remaining")
```

### Checking Lockout Status

```python
# Get lockout status for a specific user
status = db.get_lockout_status("suspicious_user")
print(f"User: {status['username']}")
print(f"  Locked: {status['status']['locked']}")
print(f"  Failed attempts: {status['status']['failed_attempts']}")
print(f"  Remaining attempts: {status['status']['remaining_attempts']}")
print(f"  Lockout count: {status['status']['lockout_count']}")
```

### Locking and Unlocking Accounts

```python
# Manually lock an account
result = db.lock_account("suspicious_user", reason="Suspicious activity detected")
print(f"Lock result: {result['success']}")

# Unlock an account
result = db.unlock_account("suspicious_user")
print(f"Unlock result: {result['success']}")
```

### Lockout Behavior

| Setting | Default |
|---------|---------|
| Max failed attempts | 5 |
| Initial lockout duration | 15 minutes |
| Progressive lockout | Doubles each time (up to 24 hours) |
| Auto-unlock | Yes, after lockout expires |

## Token Revocation

Revoke JWT tokens to immediately invalidate them before expiration.

### Revoking Current Token

```python
# Login and get a token
login_response = db.login("admin", "SecureAdmin@123!")
access_token = login_response.get("access_token")

# Revoke the current token (secure logout)
result = db.revoke_token(access_token)
print(f"Token revoked with jti: {result['jti']}")
```

### Revoking All User Tokens (Admin Only)

```python
# Revoke all tokens for a potentially compromised user
result = db.revoke_all_tokens(
    user_id="user-uuid-here",
    reason="security_incident"
)
print(f"All tokens revoked for user: {result['user_id']}")
```

### Security Incident Response

```python
def handle_security_incident(admin_db, compromised_user_ids):
    """Revoke all tokens for potentially compromised accounts."""
    for user_id in compromised_user_ids:
        admin_db.revoke_all_tokens(user_id, reason="security_incident")
        admin_db.lock_account(user_id, reason="security_investigation")
        print(f"Secured account: {user_id}")
```

## Audit Logging (Admin Only)

Query and monitor security events for compliance and debugging.

### Querying Audit Logs

```python
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

# Filter by IP address
ip_events = db.get_audit_logs(ip_address="192.168.1.100", limit=50)

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
| Token Revocation | `token_revoked`, `all_tokens_revoked` |
| Authorization | `permission_denied`, `access_granted` |
| Rate Limiting | `rate_limit_exceeded` |
| Account Lockout | `account_lockout_triggered`, `account_locked`, `account_unlocked` |

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

### Memory Management

```python
# Get memory statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance}")

# Consolidate memory
consolidated = memory.consolidate()
print(f"Consolidated {consolidated} episodes")

# Forget low-relevance episodes
forgotten = memory.forget(min_relevance=0.2)
print(f"Forgot {forgotten} episodes")

# Clear all episodes
memory.clear()
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
    AuthenticationError,
    PermissionDeniedError,
    NodeNotFoundError
)

try:
    db = QilbeeDB("http://localhost:7474")
    db.login("admin", "password")
    graph = db.graph("my_graph")

    # Your operations here
    node = graph.get_node(123)

except AuthenticationError as e:
    print(f"Authentication failed: {e}")
except PermissionDeniedError as e:
    print(f"Permission denied: {e}")
except ConnectionError as e:
    print(f"Failed to connect: {e}")
except QueryError as e:
    print(f"Query failed: {e}")
except NodeNotFoundError as e:
    print(f"Node not found: {e}")
except QilbeeDBError as e:
    print(f"Database error: {e}")
```

## API Reference

### QilbeeDB Class

Main database client.

**Connection Methods:**

| Method | Description |
|--------|-------------|
| `health()` | Get database health status |
| `close()` | Close the database connection |

**Graph Methods:**

| Method | Description |
|--------|-------------|
| `graph(name)` | Get or create graph |
| `list_graphs()` | List all graphs |
| `create_graph(name)` | Create new graph |
| `delete_graph(name)` | Delete graph |

**Authentication Methods:**

| Method | Description |
|--------|-------------|
| `login(username, password)` | Login with JWT |
| `logout()` | Logout and clear auth |
| `is_authenticated()` | Check auth status |
| `set_api_key(api_key)` | Switch to API key auth |
| `refresh_token()` | Refresh JWT access token |

**User Management Methods (Admin):**

| Method | Description |
|--------|-------------|
| `create_user(username, password, email, roles)` | Create user |
| `list_users()` | List all users |
| `get_user(user_id)` | Get user by ID |
| `update_user(user_id, password, roles)` | Update user |
| `update_user_roles(user_id, roles)` | Update user roles |
| `delete_user(user_id)` | Delete user |

**API Key Methods:**

| Method | Description |
|--------|-------------|
| `create_api_key(name)` | Create API key |
| `list_api_keys()` | List API keys |
| `delete_api_key(key_id)` | Delete API key |

**Rate Limit Methods (Admin):**

| Method | Description |
|--------|-------------|
| `create_rate_limit_policy(...)` | Create policy |
| `list_rate_limit_policies()` | List policies |
| `get_rate_limit_policy(id)` | Get policy |
| `update_rate_limit_policy(...)` | Update policy |
| `delete_rate_limit_policy(id)` | Delete policy |

**Account Lockout Methods (Admin):**

| Method | Description |
|--------|-------------|
| `get_locked_accounts()` | Get all locked accounts |
| `get_lockout_status(username)` | Get lockout status |
| `lock_account(username, reason)` | Lock account |
| `unlock_account(username)` | Unlock account |

**Token Revocation Methods:**

| Method | Description |
|--------|-------------|
| `revoke_token(token)` | Revoke specific token |
| `revoke_all_tokens(user_id, reason)` | Revoke all user tokens (Admin) |

**Audit Log Methods (Admin):**

| Method | Description |
|--------|-------------|
| `get_audit_logs(**filters)` | Query audit logs |
| `get_failed_logins(limit)` | Get failed logins |
| `get_user_audit_events(username, limit)` | Get user events |
| `get_security_events(limit)` | Get security events |

**Agent Memory Methods:**

| Method | Description |
|--------|-------------|
| `agent_memory(agent_id, config)` | Create agent memory |

### Graph Class

Graph operations.

| Method | Description |
|--------|-------------|
| `create_node(labels, properties)` | Create node |
| `get_node(node_id)` | Get node by ID |
| `update_node(node)` | Update node |
| `delete_node(node_id)` | Delete node |
| `detach_delete_node(node_id)` | Delete node with relationships |
| `find_nodes(label, limit)` | Find nodes by label |
| `create_relationship(...)` | Create relationship |
| `get_relationships(node, direction)` | Get relationships |
| `query(cypher, parameters)` | Execute Cypher query |

### Node Class

| Attribute | Description |
|-----------|-------------|
| `id` | Node ID |
| `labels` | Node labels list |
| `properties` | Node properties dict |

| Method | Description |
|--------|-------------|
| `get(key, default)` | Get property |
| `set(key, value)` | Set property |
| `__getitem__(key)` | Access property: `node["name"]` |
| `__setitem__(key, value)` | Set property: `node["age"] = 31` |

### Relationship Class

| Attribute | Description |
|-----------|-------------|
| `id` | Relationship ID |
| `type` | Relationship type |
| `start_node` | Start node ID |
| `end_node` | End node ID |
| `properties` | Relationship properties dict |

## Real-World Examples

### Social Network

```python
# Create social network
db = QilbeeDB("http://localhost:7474")
db.login("admin", "SecureAdmin@123!")
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
graph.create_relationship(
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
graph.create_relationship(python, 'USED_FOR', web_dev, {'popularity': 0.9})

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
user = graph.create_node(['Customer'], {'user_id': 'U001', 'name': 'Jane Doe'})
laptop = graph.create_node(['Product'], {
    'product_id': 'P001',
    'name': 'Laptop Pro',
    'category': 'Electronics'
})

# Track purchase
graph.create_relationship(user, 'PURCHASED', laptop, {'date': '2024-01-15', 'rating': 5})

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

## Best Practices

### Use Parameterized Queries

Always use parameters instead of string interpolation:

```python
# Good: Parameterized query
results = graph.query(
    "MATCH (p:Person) WHERE p.name = $name RETURN p",
    {"name": user_input}
)

# Bad: String interpolation (vulnerable to injection)
results = graph.query(
    f"MATCH (p:Person) WHERE p.name = '{user_input}' RETURN p"
)
```

### Use Context Managers

Always use context managers for automatic cleanup:

```python
# Good: Context manager
with QilbeeDB("http://localhost:7474") as db:
    db.login("admin", "SecureAdmin@123!")
    graph = db.graph("my_graph")
    # Operations...

# Bad: Manual management
db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_graph")
# Operations...
db.close()  # Easy to forget!
```

### Use API Keys for Applications

```python
# For automated applications, use API keys
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "api_key": os.environ.get("QILBEEDB_API_KEY")
})
```

### Handle Errors Appropriately

```python
from qilbeedb.exceptions import AuthenticationError, ConnectionError

try:
    db = QilbeeDB("http://localhost:7474")
    db.login("admin", "password")
except AuthenticationError:
    # Handle authentication failure
    print("Invalid credentials")
except ConnectionError:
    # Handle connection issues
    print("Cannot connect to database")
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

## Next Steps

- Learn about [Graph Operations](../graph-operations/nodes.md)
- Explore [Cypher Queries](../cypher/introduction.md)
- Understand [Agent Memory](../agent-memory/overview.md)
- Review [Security Features](../security/authentication.md)
- Check out [Use Cases](../use-cases/ai-agents.md)
