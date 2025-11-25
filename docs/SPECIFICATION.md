# QilbeeDB - Agent-First Graph Database Specification

## Project Overview

**QilbeeDB** is a next-generation graph database designed specifically for AI agents, implementing the best patterns from FalkorDB while solving its critical problems and limitations. Built by AICUBE TECHNOLOGY LLC, QilbeeDB aims to be the definitive knowledge graph solution for LLM-powered applications, agent memory systems, and GraphRAG implementations.

---

## 1. Executive Summary

### 1.1 Vision
QilbeeDB will be an **agent-first graph database** that provides:
- Ultra-low latency for real-time AI agent interactions
- Native bi-temporal data model for agent memory
- First-class support for episodic, semantic, procedural, and factual memory types
- Complete OpenCypher compliance without the bugs and limitations of existing solutions
- Standalone operation (no Redis dependency)

### 1.2 Key Differentiators from FalkorDB

| Aspect | FalkorDB Problem | QilbeeDB Solution |
|--------|------------------|-------------------|
| **Redis Dependency** | Requires Redis 7.4+, cannot run standalone | Standalone database with optional Redis protocol compatibility |
| **Cypher Bugs** | Multiple crash-inducing query patterns | Complete OpenCypher implementation with extensive test coverage |
| **Transaction Consistency** | Deleted nodes accessible in RETURN | Proper ACID transaction semantics |
| **Memory Model** | No native agent memory support | First-class episodic/semantic memory APIs |
| **Temporal Queries** | No bi-temporal support | Native bi-temporal data model |
| **Index Limitations** | No not-equal filter optimization | Complete index optimization including inequality |
| **LIMIT Behavior** | Eager operations ignore LIMIT | Proper LIMIT clause semantics |
| **Relationship Patterns** | Crashes with multiple path variables | Robust pattern matching engine |

---

## 2. Problem Analysis: FalkorDB Issues

### 2.1 Critical Bugs (Must Fix)

1. **Path Variable Property Assignment** (#1394)
   - SET incorrectly allows assigning properties to Path variables
   - Should reject `SET p.k1 = ...` syntax

2. **Transaction Consistency Violation** (#1393)
   - Nodes remain accessible after DELETE within same transaction
   - Violates ACID semantics

3. **Multiple Path Variables Crash** (#1353)
   - System crashes when using multiple path variables in OPTIONAL MATCH

4. **Query Engine Crashes** (#1333, #1340)
   - Crashes with multiple WHERE clauses
   - Crashes with unique properties containing whitespace
   - Signal 8 errors under various conditions

5. **DETACH DELETE Label Loss** (#1351)
   - Returned nodes lose their labels after DETACH DELETE

### 2.2 Architectural Limitations

1. **Redis Dependency**
   - Cannot run without Redis 6.2+
   - Persistence tied to Redis RDB/AOF mechanisms
   - Limited deployment flexibility

2. **Incomplete OpenCypher Support**
   - Relationship uniqueness in patterns not properly enforced
   - LIMIT clause not respected by eager operations (CREATE, SET, DELETE, MERGE)
   - Index doesn't handle not-equal (`<>`) filters
   - Cannot reuse relationship variables across patterns

3. **No Native Agent Memory**
   - No bi-temporal data model
   - No episodic memory API
   - No memory consolidation mechanisms
   - No active forgetting support

4. **Community Concerns**
   - Questions about project vitality ("FalkorDB death?" discussion)
   - SSPL license concerns for open-source claims
   - Limited community compared to Neo4j

---

## 3. QilbeeDB Architecture

### 3.1 Core Design Principles

```
┌─────────────────────────────────────────────────────────────────┐
│                        QilbeeDB Server                          │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────────┐  ┌──────────────────┐  ┌───────────────┐ │
│  │   Agent Memory   │  │   Graph Engine   │  │  Query Engine │ │
│  │     Manager      │  │                  │  │               │ │
│  ├──────────────────┤  ├──────────────────┤  ├───────────────┤ │
│  │ - Episodic Store │  │ - Sparse Matrix  │  │ - Cypher AST  │ │
│  │ - Semantic Store │  │ - GraphBLAS Ops  │  │ - Optimizer   │ │
│  │ - Procedural     │  │ - Index Manager  │  │ - Executor    │ │
│  │ - Factual        │  │ - Constraints    │  │ - Profiler    │ │
│  │ - Bi-temporal    │  │                  │  │               │ │
│  └────────┬─────────┘  └────────┬─────────┘  └───────┬───────┘ │
│           │                     │                     │         │
│  ┌────────┴─────────────────────┴─────────────────────┴───────┐ │
│  │                    Storage Engine                          │ │
│  ├────────────────────────────────────────────────────────────┤ │
│  │  - LSM Tree for Write Performance                          │ │
│  │  - B+ Tree Indexes                                         │ │
│  │  - Vector Index (HNSW)                                     │ │
│  │  - Full-Text Index (BM25)                                  │ │
│  │  - Temporal Index                                          │ │
│  │  - WAL for Durability                                      │ │
│  └────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │  Bolt Protocol  │  │  HTTP/REST API  │  │    gRPC API     │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Technology Stack

| Component | Technology | Rationale |
|-----------|------------|-----------|
| **Core Language** | Rust | Memory safety, performance, modern concurrency |
| **Matrix Operations** | GraphBLAS (SuiteSparse) | Proven sparse matrix performance |
| **Storage** | RocksDB | LSM tree, write-optimized, proven at scale |
| **Vector Index** | HNSW (custom implementation) | Fast approximate nearest neighbor |
| **Query Parser** | LALRPOP | Rust-native, excellent error messages |
| **Networking** | Tokio | Async I/O, high concurrency |
| **Serialization** | FlatBuffers | Zero-copy, schema evolution |

### 3.3 Agent Memory Subsystem

```
┌─────────────────────────────────────────────────────────────┐
│                    Agent Memory Manager                      │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────┐│
│  │                  Bi-Temporal Engine                     ││
│  │  ┌────────────────┐    ┌────────────────┐              ││
│  │  │  Event Time    │    │ Transaction    │              ││
│  │  │  (when it      │    │    Time        │              ││
│  │  │   happened)    │    │ (when stored)  │              ││
│  │  └────────────────┘    └────────────────┘              ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌───────────┐│
│  │  Episodic  │ │  Semantic  │ │ Procedural │ │  Factual  ││
│  │   Memory   │ │   Memory   │ │   Memory   │ │  Memory   ││
│  ├────────────┤ ├────────────┤ ├────────────┤ ├───────────┤│
│  │ Specific   │ │ Concepts & │ │ How-to     │ │ User      ││
│  │ events     │ │ relations  │ │ knowledge  │ │ prefs     ││
│  │ Timestamps │ │ Categories │ │ Workflows  │ │ Facts     ││
│  │ Context    │ │ Schemas    │ │ Steps      │ │ Entities  ││
│  └────────────┘ └────────────┘ └────────────┘ └───────────┘│
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │              Memory Operations                          ││
│  │  - Consolidation (STM → LTM)                           ││
│  │  - Active Forgetting (relevance decay)                 ││
│  │  - Contradiction Resolution                             ││
│  │  - Temporal Invalidation                                ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Feature Specification

### 4.1 Core Graph Database Features

#### 4.1.1 Data Model
- Property Graph Model (nodes, relationships, properties)
- Multi-graph support (10K+ tenant graphs)
- Schema-optional with schema enforcement capability
- Native JSON document support in properties

#### 4.1.2 Query Language
- **Complete OpenCypher implementation**
- All clauses: MATCH, CREATE, MERGE, DELETE, SET, REMOVE, WITH, RETURN, ORDER BY, SKIP, LIMIT, UNION, UNWIND, FOREACH, CALL
- Pattern matching with proper relationship uniqueness
- Subqueries and EXISTS/NOT EXISTS
- List comprehensions and map projections
- User-defined procedures and functions

#### 4.1.3 Indexing
- **Range Index**: B+ tree for ordered data
- **Full-Text Index**: BM25 with configurable tokenization
- **Vector Index**: HNSW for semantic search
- **Composite Index**: Multi-property indexes
- **Temporal Index**: Efficient time-range queries
- **Not-equal optimization**: Unlike FalkorDB, supports `<>` filter optimization

#### 4.1.4 Constraints
- Node key constraints
- Unique property constraints
- Existence constraints
- Relationship property existence

### 4.2 Agent-Specific Features

#### 4.2.1 Memory API

```cypher
// Create episodic memory
CREATE EPISODE (:Conversation {
  agent_id: $agent_id,
  user_message: $message,
  response: $response,
  context: $context
}) AT TIMESTAMP $event_time

// Query with temporal context
MATCH (e:Episode)
WHERE e.agent_id = $agent_id
  AND e.event_time > datetime() - duration('P7D')
RETURN e
ORDER BY e.event_time DESC

// Memory consolidation
CALL qilbee.memory.consolidate($agent_id, {
  from: 'episodic',
  to: 'semantic',
  strategy: 'summarize'
})

// Active forgetting with relevance decay
CALL qilbee.memory.forget($agent_id, {
  memory_type: 'episodic',
  older_than: duration('P30D'),
  relevance_below: 0.3
})

// Point-in-time query (bi-temporal)
MATCH (n:Entity)
AS OF TIMESTAMP $point_in_time
WHERE n.name = $name
RETURN n
```

#### 4.2.2 Hybrid Retrieval

```cypher
// Semantic + Graph + Keyword search combined
CALL qilbee.search.hybrid($query, {
  vector_weight: 0.4,
  graph_weight: 0.3,
  keyword_weight: 0.3,
  max_results: 10,
  graph_depth: 2
})
YIELD node, score, path
RETURN node, score, path

// Vector similarity with graph context
MATCH (n:Document)
WHERE qilbee.vector.similarity(n.embedding, $query_vector) > 0.8
WITH n
MATCH path = (n)-[*1..2]-(related)
RETURN n, collect(related) as context
```

#### 4.2.3 MCP Server Integration
- Native Model Context Protocol support
- Direct integration with Claude and other LLM systems
- Standardized tool definitions for graph operations

### 4.3 Performance Features

#### 4.3.1 Targets
- **Read Latency**: P99 < 10ms for single-hop queries
- **Write Throughput**: > 100K nodes/second insert
- **Vector Search**: P99 < 50ms for top-100 retrieval
- **Graph Traversal**: P99 < 100ms for 3-hop patterns

#### 4.3.2 Optimizations
- Sparse matrix operations via GraphBLAS
- Vectorized query execution
- Lazy evaluation with early termination
- Intelligent query plan caching
- Connection pooling and pipelining

### 4.4 Operational Features

#### 4.4.1 Deployment
- Standalone binary (single process)
- Embedded library mode
- Docker and Kubernetes native
- ARM64 and x86_64 support

#### 4.4.2 High Availability
- Primary-replica replication
- Automatic failover
- Read replicas for scaling
- Snapshot and incremental backup

#### 4.4.3 Observability
- OpenTelemetry integration
- Prometheus metrics endpoint
- Distributed tracing
- Query profiling and slow log

---

## 5. Protocol Support

### 5.1 Bolt Protocol (Neo4j Compatible)
- Full Bolt v4.4+ protocol implementation
- Compatible with Neo4j drivers
- WebSocket support for browser clients

### 5.2 HTTP/REST API
```
POST /db/{graph}/query
POST /db/{graph}/transaction
GET  /db/{graph}/schema
POST /db/{graph}/memory/episode
GET  /db/{graph}/memory/search
```

### 5.3 gRPC API
- High-performance binary protocol
- Streaming query results
- Bi-directional streaming for subscriptions

---

## 6. Client SDK Support

### 6.1 Official SDKs
| Language | Priority | Features |
|----------|----------|----------|
| Python | P0 | Full API, async support, LangChain integration |
| TypeScript/Node.js | P0 | Full API, async, streaming |
| Rust | P0 | Full API, zero-copy where possible |
| Go | P1 | Full API, context support |
| Java | P1 | Full API, reactive streams |

### 6.2 Framework Integrations
- LangChain / LangGraph
- LlamaIndex
- Semantic Kernel
- AutoGPT / AgentGPT
- CrewAI

---

## 7. Security

### 7.1 Authentication
- Username/password
- API key authentication
- OAuth 2.0 / OIDC integration
- mTLS for service-to-service

### 7.2 Authorization
- Role-Based Access Control (RBAC)
- Graph-level permissions
- Node/relationship label-level permissions
- Property-level access control

### 7.3 Data Protection
- Encryption at rest (AES-256)
- TLS 1.3 for transport
- Field-level encryption for sensitive properties
- Audit logging

---

## 8. Development Phases

### Phase 1: Foundation (Weeks 1-8)
- [ ] Project structure and build system
- [ ] Storage engine with RocksDB
- [ ] Basic graph operations (CRUD)
- [ ] Property Graph model implementation
- [ ] Basic Cypher parser (MATCH, CREATE, RETURN)
- [ ] Unit test framework

### Phase 2: Query Engine (Weeks 9-16)
- [ ] Complete Cypher parser
- [ ] Query optimizer
- [ ] GraphBLAS integration
- [ ] Index implementations (Range, Full-text)
- [ ] ACID transactions
- [ ] Query profiler

### Phase 3: Agent Features (Weeks 17-24)
- [ ] Bi-temporal data model
- [ ] Episodic memory API
- [ ] Vector index (HNSW)
- [ ] Hybrid search
- [ ] Memory consolidation
- [ ] Active forgetting

### Phase 4: Production Readiness (Weeks 25-32)
- [ ] Bolt protocol implementation
- [ ] HTTP/REST API
- [ ] Replication
- [ ] Security (auth, encryption)
- [ ] Observability (metrics, tracing)
- [ ] Performance optimization

### Phase 5: Ecosystem (Weeks 33-40)
- [ ] Python SDK
- [ ] TypeScript SDK
- [ ] LangChain integration
- [ ] MCP server
- [ ] Documentation
- [ ] Benchmarks and comparison

---

## 9. Competitive Positioning

### 9.1 vs FalkorDB
- No Redis dependency
- Complete OpenCypher without bugs
- Native agent memory
- Bi-temporal support
- Better stability (no crashes)

### 9.2 vs Neo4j
- Purpose-built for AI agents
- Lower latency for agent workloads
- Native vector + graph hybrid search
- Simpler deployment
- More permissive licensing

### 9.3 vs Graphiti/Zep
- Full graph database (not just memory layer)
- Complete query language
- Standalone (no external graph DB needed)
- More flexible data model

---

## 10. License

**Apache License 2.0** - Truly open source, enterprise-friendly, no SSPL restrictions.

---

## 11. Success Metrics

| Metric | Target |
|--------|--------|
| Query correctness | 100% OpenCypher TCK compliance |
| Latency | P99 < 10ms for simple queries |
| Throughput | > 100K writes/second |
| Stability | Zero crash bugs in production |
| Memory efficiency | < 2x raw data size |
| Test coverage | > 90% code coverage |

---

## 12. References

- [FalkorDB GitHub](https://github.com/FalkorDB/FalkorDB)
- [FalkorDB Documentation](https://docs.falkordb.com/)
- [Graphiti - Knowledge Graph Memory](https://github.com/getzep/graphiti)
- [OpenCypher Specification](https://opencypher.org/)
- [GraphBLAS Specification](https://graphblas.org/)
- [Zep Temporal Knowledge Graph Paper](https://arxiv.org/html/2501.13956v1)

---

*QilbeeDB Specification v1.0*
*AICUBE TECHNOLOGY LLC*
*2024*
