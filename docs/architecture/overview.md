# Architecture Overview

QilbeeDB is built with a clean, layered architecture designed for high performance, reliability, and extensibility.

## System Layers

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

## Key Components

### Storage Engine
- **RocksDB** backend for persistent storage
- **Write-Ahead Log** for durability
- **Bloom filters** for fast lookups
- **Compression** for space efficiency

### Graph Engine  
- **Multi-label nodes** with dynamic properties
- **Typed relationships** with properties
- **ACID transactions** for data integrity
- **Efficient traversals** with optimized algorithms

### Query Engine
- **OpenCypher** query language support
- **Cost-based optimization** for query planning
- **Vectorized execution** for performance
- **Query caching** for repeated queries

### Memory Engine
- **Bi-temporal tracking** (event + transaction time)
- **Automatic consolidation** from short to long-term memory
- **Relevance-based forgetting** to manage memory size
- **Four memory types**: episodic, semantic, procedural, factual

## Design Principles

1. **Performance First** - Rust implementation with zero-cost abstractions
2. **Safety** - Compile-time guarantees prevent common bugs
3. **Scalability** - Designed for high-throughput workloads
4. **Extensibility** - Plugin architecture for custom functionality

## Next Steps

- Deep dive into [Storage Engine](storage.md)
- Explore [Query Engine](query-engine.md)  
- Learn about [Memory Engine](memory-engine.md)
- Understand [Bi-Temporal Model](bi-temporal.md)
