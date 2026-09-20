

> Rust 0.8.0 adds durable scoped change feeds, revisioned memory review, transitive provenance validation and consumer checkpoints, with externally supplied embeddings and maintained Markdown user guides.
> Provision a tenant locally before use. Legacy routes require explicit opt-in
> and retain their known limitations. See the [migration and API contract](docs/api/platform-http.md).
<div align="center">

![QilbeeDB Logo](https://preview--agent-chronicle-db.lovable.app/assets/qilbee-logo-c3CsNydB.png)

**Graph Database and Evidence-Driven Memory for AI Agents**

[![License](https://img.shields.io/badge/License-BSL%201.1-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-tested%20on%201.93.1-orange.svg)](https://www.rust-lang.org/)
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

QilbeeDB is a graph database written in Rust for experimenting with persistent
agent memory, temporal records, retrieval and outcome-driven procedural learning.
It is under active development; production guarantees and benchmark leadership
have not been established.

**Implementation status:** the Rust library includes a RocksDB episode backend
and an evidence-driven procedure ledger. The default HTTP platform now provides
durable tenant credentials, exact resource grants, versioned memory commands,
idempotent receipts, conditional revisions and procedural learning with immutable
contracts, explicit evaluation outcomes and exact-baseline selection.
[Semantic retrieval](docs/api/semantic-memory.md) ranks externally generated
embeddings by cosine similarity with exact model/revision binding and explicit
scan coverage. The [learned-tool API](docs/api/learned-tools.md) stores immutable
source revisions, executor profiles and development/repair receipts. External
workers generate and test tools; tool release and invocation gates remain pending.
Cypher support is partial and Bolt is a placeholder. Graph
transactions publish entity and index mutations atomically; snapshot isolation
and conflict detection remain unimplemented.
See the [code audit and research roadmap](docs/research/agent-memory-evolution.md)
before planning production use.

## ✨ Features

### 🧠 **AI Agent Memory**
- **Episode Storage**: In-memory and RocksDB backends, with event and transaction timestamps
- **Consolidation**: Callable LLM-based summarization and fact extraction
- **Forgetting**: Callable relevance decay and pruning
- **Procedural Learning**: Immutable candidates, paired evaluation receipts,
  fixed-budget promotion and automatic suspension after monitoring failures
  ([Rust API](docs/agent-memory/learning.md))

### ⚡ **High Performance**
- **Rust Implementation** with a RocksDB storage backend
- **Vector Retrieval**: In-memory HNSW index and configurable embedding providers in the Rust library
- **Performance Validation**: Comparative latency, throughput and quality benchmarks are planned

### 📊 **OpenCypher Support**
- **Partial Cypher Support** through a simple parser and query executor
- Full language compatibility and conformance testing are planned

### 🔌 **Multiple Protocols**
- **Bolt Protocol**: Placeholder; Neo4j compatibility is not yet available
- **HTTP REST API**: Versioned identity and scoped memory with an [OpenAPI contract](docs/api/openapi.json)
- **Local Docker**: Persistent standalone deployment with [optional QMN network access](docs/operations/docker.md)
- **gRPC Support**: High-performance RPC (planned)

### 🏢 **Production Roadmap**
- Snapshot isolation, conflict handling and fault-injection recovery validation
- Learned-tool services, provenance propagation and shared change feeds
- Verified recovery, observability and reproducible benchmarks

### Run the learning cycle

```bash
cargo run -p qilbee-memory --example learning_cycle --locked
```

This offline example evaluates a procedure, promotes it, restores it after
reopening the database and suspends it after resource regressions. It is a
deterministic demonstration, not an external agent benchmark.

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
# Prerequisites: Rust with edition 2024 support (tested on 1.93.1), Git
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
- [Security](https://docs.qilbeedb.io/security/overview/)
- [Architecture](https://docs.qilbeedb.io/architecture/overview/)
- [API Reference](https://docs.qilbeedb.io/api/http-api/)

## 🛠️ Development

### Prerequisites

- Rust with edition 2024 support (tested on 1.93.1)
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

We welcome contributions from the community! Please read our [Contributing Guide](https://docs.qilbeedb.io/contributing/setup/) for details on:

- Development setup
- Code style guidelines
- Testing requirements
- [Feature validation and five-PR merge batches](docs/contributing/feature-delivery.md)

### Quick Contribution Steps

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes and add tests
4. Run tests: `cargo test`
5. Commit your changes: `git commit -m 'Add amazing feature'`
6. Push to branch: `git push origin feature/amazing-feature`
7. Open a Pull Request

## 🔒 Security

QilbeeDB includes enterprise-grade security features:

### Authentication
- **JWT (JSON Web Tokens)** - RS256 algorithm for stateless authentication
- **API Keys** - Long-lived tokens for applications and services
- **Session Management** - Configurable expiration and inactivity timeouts

### Authorization (RBAC)
- **Role-Based Access Control** - Fine-grained permission system
- **5 Predefined Roles** - Read, Developer, Analyst, Admin, SuperAdmin
- **30+ Permissions** - Granular control over all operations
- **Custom Roles** - Create roles with specific permission sets

### Rate Limiting
- **Token Bucket Algorithm** - Smooth rate control with burst allowance
- **Per-Endpoint Policies** - Different limits for different operations
- **Global Protection** - Applied to all API endpoints
- **Dynamic Management** - Modify limits at runtime via API

### Password Security
- **Argon2id Hashing** - Memory-hard algorithm resistant to GPU attacks
- **Strong Password Requirements** - Enforced complexity rules

For complete security documentation, see our [Security Guide](https://docs.qilbeedb.io/security/overview/).

If you discover a security vulnerability, please email contact@aicube.ca instead of using the issue tracker.

## 📊 Benchmarks

Reproducible performance and agent-quality baselines have not yet been
published for this implementation. The
[evaluation plan](docs/research/agent-memory-evolution.md#evaluation-protocol-for-state-of-the-art-comparisons)
defines the datasets, controls and operational measurements needed before
making comparative claims. Unit tests and the offline example validate
behavior, not production throughput or superiority over other memory systems.

## 🗺️ Roadmap

- [x] Core graph database functionality
- [x] Partial Cypher query execution
- [x] Event and transaction timestamps on episodes
- [x] HTTP REST API
- [ ] Bolt protocol implementation and conformance
- [x] Python SDK
- [x] Security components (JWT, API keys, RBAC)
- [ ] Secure production bootstrap and resource ownership across API routes
- [x] Persistent HTTP episode storage with abrupt-restart regression coverage
- [x] Versioned idempotent memory API with conditional revisions and authenticated tenant/sharing scopes
- [ ] Procedural HTTP API, durable change feeds and indexed hybrid retrieval
- [x] Atomic multi-operation graph entity and index commits
- [ ] Snapshot isolation and transaction conflict detection
- [x] BM25 lexical ranking and deterministic hybrid retrieval
- [x] Scoped episode writes and invalidation-aware reads
- [x] Durable episode integrity and versioned structured payloads
- [x] Outcome-driven procedure ledger in the Rust library
- [x] Rate limiting with token bucket algorithm
- [ ] Audit logging
- [ ] Distributed clustering
- [ ] Graph algorithms library
- [ ] Real-time streaming
- [ ] GraphQL API
- [ ] Additional language SDKs (JavaScript, Java, Go)
- [ ] Cloud-native deployment tools

## 📄 License

QilbeeDB is licensed under the **Business Source License 1.1 (BSL 1.1)**. See [LICENSE](LICENSE) for full details.

### Key License Terms

- **Licensor**: AICUBE TECHNOLOGY LLC (Delaware, USA)
- **Change Date**: January 1, 2029
- **Change License**: Apache License, Version 2.0

### What You CAN Do

- Use QilbeeDB for internal business operations
- Develop applications that embed QilbeeDB
- Modify and create derivative works for internal use
- Use for research, education, and non-commercial purposes
- Distribute to contractors working on your behalf

### What You CANNOT Do (Without Commercial License)

- Offer QilbeeDB as a Database-as-a-Service (DBaaS)
- Sell, lease, or sublicense as a standalone database product
- Provide database hosting services to third parties

### Commercial Licensing

For uses not permitted under BSL 1.1, commercial licenses are available.
Contact: licensing@aicube.ca

```
Copyright (c) 2024-2025 AICUBE TECHNOLOGY LLC. All Rights Reserved.
Licensed under the Business Source License 1.1
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
Experiment with native episode storage, retrieval and evidence-driven procedure selection. Agent execution and evaluation remain application responsibilities.

### For Graph Database Users
Explore a Rust graph engine with partial Cypher support. Neo4j/Bolt compatibility is a future goal.

### For Enterprises
Evaluate the implementation against the documented production gaps and research roadmap before deployment.

---

<div align="center">

**Built with ❤️ by [AICUBE TECHNOLOGY LLC](https://www.aicube.ca/)**

[Website](https://qilbeedb.io/) •
[Documentation](https://docs.qilbeedb.io/) •
[GitHub](https://github.com/aicubetechnology/qilbeeDB) •
[Docker Hub](https://hub.docker.com/r/qilbeedb/qilbeedb)

If you find QilbeeDB useful, please give us a ⭐️ on GitHub!

</div>
