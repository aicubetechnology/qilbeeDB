# Client Libraries Overview

QilbeeDB provides official client libraries for multiple programming languages, making it easy to integrate QilbeeDB into your applications.

## Available Client Libraries

### Python SDK

The Python SDK is the most mature and feature-complete client library for QilbeeDB.

**Features:**
- Full graph operations (nodes, relationships, properties)
- Complete Cypher query support
- Native agent memory management
- Query builder for programmatic query construction
- Connection pooling and management
- Comprehensive error handling

**Documentation:** [Python SDK](python.md)

**Installation:**
```bash
pip install qilbeedb
```

**Quick Example:**
```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_graph")

# Create nodes and relationships
user = graph.create_node(['User'], {'name': 'Alice'})
```

### JavaScript/TypeScript SDK

*Coming soon*

### Java SDK

*Coming soon*

### Go SDK

*Coming soon*

### Rust SDK

*Coming soon*

## Protocol Support

QilbeeDB supports multiple protocols for maximum interoperability:

### HTTP REST API

The HTTP REST API provides a simple, stateless interface for all QilbeeDB operations.

- **Endpoint:** `http://localhost:7474`
- **Format:** JSON
- **Authentication:** Optional (configurable)

**Example:**
```bash
curl -X POST http://localhost:7474/graphs/my_graph/query \
  -H "Content-Type: application/json" \
  -d '{"cypher": "MATCH (n:User) RETURN n"}'
```

### Bolt Protocol

Bolt is a binary protocol designed for graph databases (Neo4j-compatible).

- **Endpoint:** `bolt://localhost:7687`
- **Format:** Binary
- **Features:** Streaming, pipelining, type system

### gRPC

High-performance RPC protocol for microservices architectures.

- **Endpoint:** `grpc://localhost:7688`
- **Format:** Protocol Buffers
- **Features:** Bidirectional streaming, strong typing

## Choosing a Client Library

### Use Python SDK When:
- Building AI agents with memory requirements
- Rapid prototyping and development
- Data science and analytics workloads
- Backend services in Python

### Use HTTP REST API When:
- Language without native SDK
- Stateless request/response pattern
- Simple integration requirements
- Testing and debugging

### Use Bolt Protocol When:
- Need Neo4j compatibility
- High-performance requirements
- Streaming large result sets
- Existing Bolt infrastructure

### Use gRPC When:
- Microservices architecture
- Low-latency requirements
- Bidirectional streaming needs
- Strong typing requirements

## Connection Management

All client libraries support connection pooling and configuration:

```python
# Python example
from qilbeedb import QilbeeDB

db = QilbeeDB(
    "http://localhost:7474",
    max_connections=10,
    timeout=30
)
```

Learn more: [Connection Management](connections.md)

## Authentication

QilbeeDB supports multiple authentication methods:

- **None** - Open access (development only)
- **Basic Auth** - Username/password
- **Token-based** - JWT tokens
- **OAuth** - Third-party authentication

```python
# Example with authentication
db = QilbeeDB(
    "http://localhost:7474",
    auth=("username", "password")
)
```

## Error Handling

All client libraries provide structured error handling:

```python
from qilbeedb.exceptions import (
    ConnectionError,
    QueryError,
    NodeNotFoundError
)

try:
    result = graph.query("MATCH (n) RETURN n")
except ConnectionError as e:
    print(f"Connection failed: {e}")
except QueryError as e:
    print(f"Query failed: {e}")
```

## Next Steps

- Explore the [Python SDK](python.md) documentation
- Learn about [Connection Management](connections.md)
- Read the [HTTP REST API](../api/http-api.md) reference
- Check out the [Bolt Protocol](../api/bolt-protocol.md) documentation

## Community SDKs

Community-maintained SDKs are welcome! If you've built a client library for QilbeeDB, please let us know.

## Contributing

Interested in building a client library? Check out our [contributing guide](../contributing/setup.md) and API documentation.
