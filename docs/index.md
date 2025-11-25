# QilbeeDB Documentation

**Enterprise-Grade Graph Database with Bi-Temporal Agent Memory**

Welcome to the QilbeeDB documentation! QilbeeDB is a high-performance graph database written in Rust, designed specifically for AI agent systems with advanced bi-temporal memory management.

## What is QilbeeDB?

QilbeeDB combines the power of graph databases with sophisticated memory architectures to enable AI agents to maintain context, learn from interactions, and evolve over time. It's the first graph database to natively integrate bi-temporal memory management (event time + transaction time) with support for episodic, semantic, procedural, and factual memory types.

## Key Features

### 🚀 High Performance
Built in Rust for maximum performance and safety. Zero-cost abstractions and compile-time guarantees ensure your data operations are blazing fast.

### 🧠 Agent Memory
Native support for AI agent memory with automatic consolidation, relevance decay, and active forgetting. Perfect for building intelligent systems that learn and adapt.

### 🔄 Bi-Temporal Tracking
Track both when events occurred (event time) and when they were recorded (transaction time) for complete auditability and time-travel queries.

### 📊 OpenCypher Support
Full OpenCypher query language support with enterprise-grade query execution engine featuring cost-based optimization and vectorized execution.

### ⚡ RocksDB Backend
Leverages RocksDB for high-performance storage with compression, bloom filters, and write-ahead logging.

### 🔌 Multiple Protocols
Bolt (Neo4j-compatible), HTTP REST API, and gRPC support for maximum interoperability.

## Quick Example

```python
from qilbeedb import QilbeeDB

# Connect to QilbeeDB
db = QilbeeDB("http://localhost:7474")

# Create a graph
graph = db.graph("my_social_network")

# Create nodes
alice = graph.create_node(
    ['Person', 'User'],
    {'name': 'Alice', 'age': 30}
)

bob = graph.create_node(
    ['Person', 'User'],
    {'name': 'Bob', 'age': 35}
)

# Create relationship
friendship = graph.create_relationship(
    alice, 'KNOWS', bob,
    {'since': '2020-01-15'}
)

# Query with Cypher
results = graph.query("""
    MATCH (p:Person)-[:KNOWS]->(friend)
    WHERE p.name = $name
    RETURN friend.name, friend.age
""", {"name": "Alice"})
```

## Use Cases

QilbeeDB excels in scenarios requiring both graph relationships and intelligent memory management:

- **AI Agent Systems**: Customer service bots, personal assistants, autonomous agents
- **Social Networks**: Friend graphs, influence networks, community detection
- **Knowledge Graphs**: Semantic knowledge management, concept relationships
- **Recommendation Systems**: Collaborative filtering, personalized recommendations
- **Multi-Agent Coordination**: Agent collaboration, shared knowledge bases

## Getting Started

Ready to dive in? Here's how to get started:

1. **[Installation](getting-started/installation.md)** - Install QilbeeDB on your system
2. **[Quick Start](getting-started/quickstart.md)** - Your first QilbeeDB application
3. **[Python SDK](client-libraries/python.md)** - Detailed SDK documentation
4. **[Cypher Queries](cypher/introduction.md)** - Learn the query language

## Architecture Overview

QilbeeDB is built with a clean, layered architecture:

```
┌─────────────────────────────────────────┐
│     Protocol Layer                       │
│   Bolt | HTTP/REST | gRPC                │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│     Query Engine                         │
│   Parser → Planner → Optimizer → Executor│
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│     Graph Engine                         │
│   Nodes | Relationships | Transactions   │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│     Memory Engine                        │
│   Episodic | Semantic | Procedural       │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│     Storage Engine                       │
│   RocksDB | Indexes | WAL                │
└─────────────────────────────────────────┘
```

## Why QilbeeDB?

### For AI Applications
- **Native Memory Management**: Built-in episodic, semantic, and procedural memory
- **Bi-Temporal Tracking**: Full audit trail with event and transaction time
- **Automatic Consolidation**: Short-term to long-term memory consolidation
- **Relevance-Based Forgetting**: Intelligent memory pruning

### For Graph Applications
- **High Performance**: Rust-powered performance with RocksDB storage
- **OpenCypher**: Industry-standard query language
- **ACID Transactions**: Full transactional support
- **Flexible Schema**: Multi-label nodes and dynamic properties

### For Enterprise
- **Production-Ready**: Enterprise-grade query execution engine
- **Multiple Protocols**: Bolt, HTTP REST, gRPC support
- **Monitoring**: Prometheus metrics, distributed tracing
- **Scalable**: Designed for high-throughput workloads

## Community & Support

- **GitHub**: [qilbeedb/qilbeedb](https://github.com/aicubetechnology/qilbeeDB)
- **Documentation**: You're reading it!
- **Issues**: Report bugs and request features on GitHub
- **Contributing**: See our [contributing guide](contributing/setup.md)

## License

QilbeeDB is licensed under the Apache License 2.0. See the LICENSE file for details.

---

**Ready to build something amazing?** Start with our [installation guide](getting-started/installation.md)!
