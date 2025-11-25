# Connection Management

Learn how to connect to QilbeeDB, manage connection pools, and configure client settings for optimal performance.

## Basic Connection

### Python SDK

```python
from qilbeedb import QilbeeDB

# Simple connection
db = QilbeeDB("http://localhost:7474")

# Use context manager for automatic cleanup
with QilbeeDB("http://localhost:7474") as db:
    graph = db.graph("my_graph")
    # Operations...
# Connection automatically closed
```

### HTTP REST API

```bash
# Direct HTTP requests
curl http://localhost:7474/health

# Check server status
curl http://localhost:7474/graphs
```

## Connection Configuration

### Connection URL Formats

**HTTP:**
```python
db = QilbeeDB("http://localhost:7474")
db = QilbeeDB("https://qilbeedb.example.com:7474")
```

**Bolt (Coming Soon):**
```python
db = QilbeeDB("bolt://localhost:7687")
```

### Connection Options

```python
db = QilbeeDB(
    url="http://localhost:7474",
    timeout=30,              # Request timeout in seconds
    max_retries=3,           # Retry failed requests
    verify_ssl=True,         # Verify SSL certificates
)
```

## Authentication

### Basic Authentication

```python
db = QilbeeDB(
    "http://localhost:7474",
    auth=("username", "password")
)
```

### Token Authentication

```python
db = QilbeeDB(
    "http://localhost:7474",
    token="your-api-token"
)
```

## Connection Pooling

*This page is under development. Full documentation coming soon.*

Connection pooling helps manage multiple concurrent connections efficiently:

```python
# Configure connection pool
db = QilbeeDB(
    "http://localhost:7474",
    pool_size=10,           # Max connections
    pool_timeout=30         # Wait time for available connection
)
```

## Error Handling

### Connection Errors

```python
from qilbeedb import QilbeeDB
from qilbeedb.exceptions import ConnectionError

try:
    db = QilbeeDB("http://localhost:7474")
    db.list_graphs()
except ConnectionError as e:
    print(f"Failed to connect: {e}")
    # Handle connection failure
```

### Timeout Handling

```python
from qilbeedb.exceptions import TimeoutError

try:
    result = graph.query("MATCH (n) RETURN n", timeout=5)
except TimeoutError:
    print("Query timed out")
```

## Connection Best Practices

### 1. Use Context Managers

Always use context managers for automatic resource cleanup:

```python
# Good: Automatic cleanup
with QilbeeDB("http://localhost:7474") as db:
    graph = db.graph("my_graph")
    # Operations...

# Bad: Manual management
db = QilbeeDB("http://localhost:7474")
# Operations...
db.close()  # Easy to forget!
```

### 2. Reuse Connections

Create one connection and reuse it:

```python
# Good: Reuse connection
db = QilbeeDB("http://localhost:7474")
graph1 = db.graph("graph1")
graph2 = db.graph("graph2")

# Bad: Multiple connections
db1 = QilbeeDB("http://localhost:7474")
db2 = QilbeeDB("http://localhost:7474")
```

### 3. Configure Timeouts

Set appropriate timeouts for your use case:

```python
# Long-running analytics
db = QilbeeDB("http://localhost:7474", timeout=300)

# Interactive queries
db = QilbeeDB("http://localhost:7474", timeout=10)
```

### 4. Handle Connection Failures

Always handle connection failures gracefully:

```python
from qilbeedb.exceptions import ConnectionError
import time

def connect_with_retry(url, max_attempts=3):
    for attempt in range(max_attempts):
        try:
            return QilbeeDB(url)
        except ConnectionError as e:
            if attempt < max_attempts - 1:
                time.sleep(2 ** attempt)  # Exponential backoff
            else:
                raise
```

## Health Checks

Check if QilbeeDB is healthy:

```python
# Python
try:
    db = QilbeeDB("http://localhost:7474")
    # Connection successful
except ConnectionError:
    # Server not available
    pass
```

```bash
# HTTP
curl http://localhost:7474/health
```

## Next Steps

- Learn about the [Python SDK](python.md)
- Read the [Client Libraries Overview](overview.md)
- Explore [Configuration Options](../getting-started/configuration.md)
