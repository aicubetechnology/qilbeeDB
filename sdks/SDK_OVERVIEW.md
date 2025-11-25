# QilbeeDB SDKs Overview

Official client libraries for QilbeeDB in Python and Node.js/TypeScript.

## Python SDK

**Location:** `/sdks/python/`

### Installation

```bash
pip install qilbeedb
```

### Features

- ✅ Full graph operations (CRUD for nodes and relationships)
- ✅ OpenCypher query support
- ✅ Query builder with fluent interface
- ✅ Bi-temporal agent memory management
- ✅ Episode storage and retrieval
- ✅ Memory consolidation and forgetting
- ✅ HTTP/REST protocol support
- ✅ Context manager support
- ✅ Comprehensive error handling
- ✅ Type hints throughout

### Files

```
python/
├── qilbeedb/
│   ├── __init__.py          # Package exports
│   ├── client.py            # Main QilbeeDB client
│   ├── graph.py             # Graph, Node, Relationship classes
│   ├── memory.py            # AgentMemory, Episode classes
│   ├── query.py             # Query builder and result handling
│   └── exceptions.py        # Custom exceptions
├── setup.py                 # Package setup
└── README.md                # Documentation
```

### Quick Example

```python
from qilbeedb import QilbeeDB, Episode

# Connect
db = QilbeeDB("http://localhost:7474")

# Graph operations
graph = db.graph("social")
alice = graph.create_node(["Person"], {"name": "Alice", "age": 30})

# Agent memory
memory = db.agent_memory("agent-001")
ep = Episode.observation("agent-001", "User logged in")
memory.store_episode(ep)
```

## Node.js/TypeScript SDK

**Location:** `/sdks/nodejs/`

### Installation

```bash
npm install @qilbeedb/client
```

### Features

- ✅ Full TypeScript support with type definitions
- ✅ Full graph operations (CRUD for nodes and relationships)
- ✅ OpenCypher query support
- ✅ Query builder with fluent interface
- ✅ Bi-temporal agent memory management
- ✅ Episode storage and retrieval
- ✅ Memory consolidation and forgetting
- ✅ HTTP/REST protocol support
- ✅ Promise-based async API
- ✅ Comprehensive error handling
- ✅ ES6+ features

### Files

```
nodejs/
├── src/
│   ├── index.ts             # Package exports
│   ├── client.ts            # Main QilbeeDB client
│   ├── graph.ts             # Graph, Node, Relationship classes
│   ├── memory.ts            # AgentMemory, Episode classes
│   ├── query.ts             # Query builder and result handling
│   ├── exceptions.ts        # Custom exceptions
│   └── types.ts             # Common types
├── package.json             # NPM package configuration
├── tsconfig.json            # TypeScript configuration
└── README.md                # Documentation
```

### Quick Example

```typescript
import { QilbeeDB, Episode } from '@qilbeedb/client';

// Connect
const db = new QilbeeDB('http://localhost:7474');

// Graph operations
const graph = await db.graph('social');
const alice = await graph.createNode(['Person'], { name: 'Alice', age: 30 });

// Agent memory
const memory = db.agentMemory('agent-001');
const ep = Episode.observation('agent-001', 'User logged in');
await memory.storeEpisode(ep);
```

## Feature Comparison

| Feature | Python | Node.js |
|---------|--------|---------|
| Graph Operations | ✅ | ✅ |
| Cypher Queries | ✅ | ✅ |
| Query Builder | ✅ | ✅ |
| Agent Memory | ✅ | ✅ |
| HTTP Protocol | ✅ | ✅ |
| Bolt Protocol | ⏳ | ⏳ |
| TypeScript | N/A | ✅ |
| Type Hints | ✅ | ✅ |
| Async/Await | ✅ | ✅ |
| Error Handling | ✅ | ✅ |
| Documentation | ✅ | ✅ |

Legend: ✅ Implemented | ⏳ Planned

## Common API Patterns

Both SDKs follow similar patterns for consistency:

### Connection

**Python:**
```python
db = QilbeeDB("http://localhost:7474")
with QilbeeDB("http://localhost:7474") as db:
    # use db
```

**Node.js:**
```typescript
const db = new QilbeeDB('http://localhost:7474');
await db.close();
```

### Graph Operations

**Python:**
```python
graph = db.graph("social")
node = graph.create_node(["Person"], {"name": "Alice"})
result = graph.query("MATCH (p:Person) RETURN p", {"param": value})
```

**Node.js:**
```typescript
const graph = await db.graph('social');
const node = await graph.createNode(['Person'], { name: 'Alice' });
const result = await graph.query('MATCH (p:Person) RETURN p', { param: value });
```

### Agent Memory

**Python:**
```python
memory = db.agent_memory("agent-001")
ep = Episode.conversation("agent-001", "Hello", "Hi there")
memory.store_episode(ep)
recent = memory.get_recent_episodes(10)
```

**Node.js:**
```typescript
const memory = db.agentMemory('agent-001');
const ep = Episode.conversation('agent-001', 'Hello', 'Hi there');
await memory.storeEpisode(ep);
const recent = await memory.getRecentEpisodes(10);
```

## Error Handling

Both SDKs provide consistent error hierarchies:

- `QilbeeDBError` - Base exception
- `ConnectionError` - Connection failures
- `AuthenticationError` - Auth failures
- `QueryError` - Query execution errors
- `TransactionError` - Transaction errors
- `MemoryError` - Memory operation errors
- `GraphNotFoundError` - Graph not found
- `NodeNotFoundError` - Node not found
- `RelationshipNotFoundError` - Relationship not found

## Development

### Python SDK Development

```bash
cd sdks/python
pip install -e .[dev]
pytest tests/
black qilbeedb/
flake8 qilbeedb/
```

### Node.js SDK Development

```bash
cd sdks/nodejs
npm install
npm run build
npm test
npm run lint
```

## Publishing

### Python SDK

```bash
cd sdks/python
python setup.py sdist bdist_wheel
twine upload dist/*
```

### Node.js SDK

```bash
cd sdks/nodejs
npm run build
npm publish
```

## Support

- **Documentation:** https://docs.qilbeedb.com
- **Issues:** https://github.com/your-org/qilbeedb/issues
- **Email:** support@qilbeedb.com

## License

Both SDKs are licensed under Apache License 2.0, same as QilbeeDB.
