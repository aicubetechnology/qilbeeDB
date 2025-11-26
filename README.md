
<div align="center">

![QilbeeDB Logo](https://preview--agent-chronicle-db.lovable.app/assets/qilbee-logo-c3CsNydB.png)

**Enterprise-Grade Graph Database with Bi-Temporal Agent Memory**

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Documentation](https://img.shields.io/badge/docs-online-green.svg)](https://docs.qilbeedb.io/)
[![GitHub](https://img.shields.io/github/stars/aicubetechnology/qilbeeDB?style=social)](https://github.com/aicubetechnology/qilbeeDB)

Created by **[AICUBE TECHNOLOGY LLC](https://www.aicube.ca/)**

[Features](#-features) •
[Quick Start](#-quick-start) •
[Documentation](https://docs.qilbeedb.io/) •
[Examples](#-examples) •
[Contributing](#-contributing)

</div>

---

## 🚀 What is QilbeeDB?

QilbeeDB is a high-performance graph database written in Rust, designed specifically for AI agent systems with advanced bi-temporal memory management. It combines the power of graph databases with sophisticated memory architectures to enable AI agents to maintain context, learn from interactions, and evolve over time.

**The first graph database to natively integrate bi-temporal memory management** (event time + transaction time) with support for episodic, semantic, procedural, and factual memory types.

## ✨ Features

### 🧠 **AI Agent Memory**
- **Native Memory Types**: Episodic, semantic, procedural, and factual memory
- **Automatic Consolidation**: Short-term to long-term memory transitions
- **Active Forgetting**: Relevance-based memory pruning
- **Bi-Temporal Tracking**: Track both event time and transaction time

### ⚡ **High Performance**
- **Rust-Powered**: Zero-cost abstractions and memory safety
- **RocksDB Backend**: High-performance storage with compression and bloom filters
- **Vectorized Execution**: SIMD-optimized query processing
- **Cost-Based Optimization**: Intelligent query planning

### 📊 **OpenCypher Support**
- **Full Query Language**: Complete OpenCypher implementation
- **Pattern Matching**: Complex graph pattern queries
- **Aggregations**: COUNT, SUM, AVG, MIN, MAX
- **Path Finding**: Variable-length path traversal

### 🔌 **Multiple Protocols**
- **Bolt Protocol**: Neo4j-compatible for existing tools
- **HTTP REST API**: RESTful JSON interface
- **gRPC Support**: High-performance RPC (planned)

### 🏢 **Enterprise-Ready**
- **ACID Transactions**: Full transactional support
- **Query Optimization**: Cost-based query planner
- **Monitoring**: Prometheus metrics and distributed tracing
- **Production-Grade**: Battle-tested query execution engine

## 📦 Installation

### Using Docker (Recommended)

```bash
# Pull the latest image
docker pull qilbeedb/qilbeedb:latest

# Run QilbeeDB
docker run -d \
  --name qilbeedb \
  -p 7474:7474 \
  -p 7687:7687 \
  -v qilbeedb-data:/data \
  qilbeedb/qilbeedb:latest
```

### Docker Compose

```yaml
version: '3.8'

services:
  qilbeedb:
    image: qilbeedb/qilbeedb:latest
    ports:
      - "7474:7474"  # HTTP REST API
      - "7687:7687"  # Bolt Protocol
    volumes:
      - qilbeedb-data:/data
    environment:
      - QILBEE_LOG_LEVEL=info
    restart: unless-stopped

volumes:
  qilbeedb-data:
```

### Building from Source

```bash
# Prerequisites: Rust 1.70+, Git
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeeDB

# Build in release mode
cargo build --release

# Run the server
./target/release/qilbee-server
```

### Python SDK

```bash
pip install qilbeedb
```

## 🎯 Quick Start

### Creating a Graph

```python
from qilbeedb import QilbeeDB

# Connect to QilbeeDB
db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_social_network")

# Create nodes
alice = graph.create_node(
    ['Person', 'User'],
    {'name': 'Alice', 'age': 30, 'city': 'San Francisco'}
)

bob = graph.create_node(
    ['Person', 'User'],
    {'name': 'Bob', 'age': 35, 'city': 'New York'}
)

# Create relationship
friendship = graph.create_relationship(
    alice, 'KNOWS', bob,
    {'since': '2020-01-15', 'strength': 0.8}
)

# Query with Cypher
results = graph.query("""
    MATCH (p:Person)-[:KNOWS]->(friend)
    WHERE p.name = $name
    RETURN friend.name, friend.age
""", {"name": "Alice"})

for row in results:
    print(f"{row['friend.name']}, age {row['friend.age']}")
```

### Using Agent Memory

```python
from qilbeedb.memory import Episode

# Get agent memory manager
memory = db.agent_memory('customer_service_bot')

# Store conversation episodes
episode = Episode.conversation(
    'customer_service_bot',
    'Hi, I need help with my order',
    'Hello! I\'d be happy to help. What\'s your order number?'
)
memory.store_episode(episode)

# Retrieve recent conversations
recent = memory.get_recent_episodes(10)

# Get memory statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
```

## 📚 Examples

### Social Network

```python
# Find common friends
results = graph.query("""
    MATCH (alice:User {name: $alice})-[:KNOWS]->(common)<-[:KNOWS]-(bob:User {name: $bob})
    RETURN common.name
""", {"alice": "Alice", "bob": "Bob"})
```

### Knowledge Graph

```python
# Semantic relationships
results = graph.query("""
    MATCH (concept:Concept)-[:RELATES_TO*1..3]->(related:Concept)
    WHERE concept.name = $topic
    RETURN DISTINCT related.name, related.category
""", {"topic": "Machine Learning"})
```

### Recommendation System

```python
# Collaborative filtering
results = graph.query("""
    MATCH (user:User {id: $user_id})-[:PURCHASED]->(product)<-[:PURCHASED]-(other:User)
    MATCH (other)-[:PURCHASED]->(recommendation)
    WHERE NOT (user)-[:PURCHASED]->(recommendation)
    RETURN recommendation.name, COUNT(*) as score
    ORDER BY score DESC
    LIMIT 10
""", {"user_id": 12345})
```

## 🏗️ Architecture

QilbeeDB is built with a clean, layered architecture:

```
┌─────────────────────────────────────────┐
│           Protocol Layer                │
│       Bolt | HTTP/REST | gRPC           │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│             Query Engine                │
│  Parser → Planner → Optimizer → Executor│
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│             Graph Engine                │
│   Nodes | Relationships | Transactions  │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│             Memory Engine               │
│   Episodic | Semantic | Procedural      │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│           Storage Engine                │
│        RocksDB | Indexes | WAL          │
└─────────────────────────────────────────┘
```

### Core Components

- **qilbee-core**: Core data structures and types
- **qilbee-storage**: RocksDB storage layer with bi-temporal support
- **qilbee-graph**: Graph operations (nodes, relationships, transactions)
- **qilbee-query**: Query engine (parser, planner, executor)
- **qilbee-memory**: Agent memory management
- **qilbee-protocol**: Bolt and HTTP protocol implementations
- **qilbee-server**: Server orchestration and APIs

## 🎓 Use Cases

QilbeeDB excels in scenarios requiring both graph relationships and intelligent memory management:

- **AI Agent Systems**: Customer service bots, personal assistants, autonomous agents
- **Social Networks**: Friend graphs, influence networks, community detection
- **Knowledge Graphs**: Semantic knowledge management, concept relationships
- **Recommendation Systems**: Collaborative filtering, personalized recommendations
- **Multi-Agent Coordination**: Agent collaboration, shared knowledge bases
- **Fraud Detection**: Pattern recognition in transaction networks
- **Network Analysis**: Infrastructure monitoring, dependency tracking

## 📖 Documentation

Comprehensive documentation is available at:
**[https://docs.qilbeedb.io/](https://docs.qilbeedb.io/)**

### Key Sections:
- [Installation Guide](https://docs.qilbeedb.io/getting-started/installation/)
- [Quick Start](https://docs.qilbeedb.io/getting-started/quickstart/)
- [Python SDK](https://docs.qilbeedb.io/client-libraries/python/)
- [Cypher Query Language](https://docs.qilbeedb.io/cypher/introduction/)
- [Agent Memory](https://docs.qilbeedb.io/agent-memory/overview/)
- [Architecture](https://docs.qilbeedb.io/architecture/overview/)
- [API Reference](https://docs.qilbeedb.io/api/http-api/)

## 🛠️ Development

### Prerequisites

- Rust 1.70 or later
- Git
- Build tools (gcc/clang, make)

### Building

```bash
# Clone repository
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeeDB

# Build all crates
cargo build

# Run tests
cargo test

# Build documentation
cargo doc --no-deps --open
```

### Project Structure

```
qilbeeDB/
├── crates/
│   ├── qilbee-core/        # Core types and traits
│   ├── qilbee-storage/     # Storage layer (RocksDB)
│   ├── qilbee-graph/       # Graph operations
│   ├── qilbee-query/       # Query engine
│   ├── qilbee-memory/      # Agent memory
│   ├── qilbee-protocol/    # Protocol implementations
│   └── qilbee-server/      # Server application
├── sdks/
│   └── python/             # Python SDK
├── docs/                   # Documentation (MkDocs)
└── examples/               # Usage examples
```

## 🤝 Contributing

We welcome contributions from the community! Please read our [https://docs.qilbeedb.io/getting-started/installation/) for details on:

- Development setup
- Code style guidelines
- Testing requirements
- Pull request process

### Quick Contribution Steps

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes and add tests
4. Run tests: `cargo test`
5. Commit your changes: `git commit -m 'Add amazing feature'`
6. Push to branch: `git push origin feature/amazing-feature`
7. Open a Pull Request

## 🔒 Security

Security is a top priority for QilbeeDB. If you discover a security vulnerability, please email contact@aicube.ca instead of using the issue tracker.

## 📊 Benchmarks

QilbeeDB is designed for high performance:

- **Node Creation**: 100,000+ nodes/second
- **Relationship Creation**: 50,000+ relationships/second
- **Simple Queries**: Sub-millisecond response times
- **Complex Pattern Matching**: Optimized with cost-based planning
- **Memory Consolidation**: Real-time processing with minimal overhead

See our [benchmark documentation](https://docs.qilbeedb.io/operations/performance/#use-parameters) for detailed performance metrics.

## 🗺️ Roadmap

- [x] Core graph database functionality
- [x] OpenCypher query language support
- [x] Bi-temporal memory management
- [x] HTTP REST API
- [x] Bolt protocol support
- [x] Python SDK
- [ ] Distributed clustering
- [ ] Graph algorithms library
- [ ] Real-time streaming
- [ ] GraphQL API
- [ ] Additional language SDKs (JavaScript, Java, Go)
- [ ] Cloud-native deployment tools

## 📄 License

QilbeeDB is licensed under the Apache License 2.0. See [LICENSE](LICENSE) for details.

```
Copyright 2024 AICUBE TECHNOLOGY LLC

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

## 🙏 Acknowledgments

QilbeeDB is built on top of excellent open-source projects:

- [RocksDB](https://rocksdb.org/) - High-performance storage engine
- [Tokio](https://tokio.rs/) - Asynchronous runtime
- [Serde](https://serde.rs/) - Serialization framework

## 💬 Community & Support

- **Documentation**: [https://docs.qilbeedb.io/](https://docs.qilbeedb.io/)
- **GitHub Issues**: [Report bugs or request features](https://github.com/aicubetechnology/qilbeeDB/issues)
- **Discussions**: Join our GitHub Discussions for Q&A and community support
- **Email**: contact@aicube.ca

## 🌟 Why QilbeeDB?

### For AI Developers
Built specifically for AI agents with native memory management. No need to build complex memory systems on top of generic databases.

### For Graph Database Users
Familiar OpenCypher syntax with modern Rust performance. Drop-in replacement for Neo4j-compatible tools via Bolt protocol.

### For Enterprises
Production-ready with ACID transactions, monitoring, and enterprise-grade query optimization. Designed for high-throughput workloads.

---

<div align="center">

**Built with ❤️ by [AICUBE TECHNOLOGY LLC](https://www.aicube.ca/)**

[Website](https://qilbeedb.io/) •
[Documentation](https://docs.qilbeedb.io/) •
[GitHub](https://github.com/aicubetechnology/qilbeeDB) •
[Docker Hub](https://hub.docker.com/r/qilbeedb/qilbeedb)

If you find QilbeeDB useful, please give us a ⭐️ on GitHub!

</div>
