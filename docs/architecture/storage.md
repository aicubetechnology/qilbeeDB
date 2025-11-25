# Storage Engine

QilbeeDB's storage engine is built on RocksDB, providing high-performance persistent storage with ACID guarantees.

## Architecture

```
┌─────────────────────────────────────┐
│     Graph & Memory APIs             │
└─────────────────────────────────────┘
┌─────────────────────────────────────┐
│     Storage Layer                   │
│  ┌─────────┬─────────┬───────────┐  │
│  │ Nodes   │  Rels   │  Props    │  │
│  └─────────┴─────────┴───────────┘  │
└─────────────────────────────────────┘
┌─────────────────────────────────────┐
│     RocksDB                         │
│  • LSM Trees                        │
│  • Write-Ahead Log                  │
│  • Bloom Filters                    │
│  • Compression                      │
└─────────────────────────────────────┘
```

## Key Features

### RocksDB Backend

QilbeeDB uses RocksDB as its persistence layer:

- **LSM Trees** - Efficient write performance
- **Block Cache** - Fast reads with configurable cache size
- **Compression** - LZ4/Snappy compression reduces storage
- **Write-Ahead Log** - Durability guarantees

### Data Organization

Data is organized into separate column families:

```rust
// Column families
- nodes:           NodeId -> Node data
- relationships:   RelationshipId -> Relationship data
- properties:      EntityId -> Properties
- node_labels:     Label -> Set<NodeId>
- indexes:         Index key -> EntityId
```

### Indexing

Automatic and custom indexes for fast lookups:

**Label Index:**
```cypher
// Automatically indexed
MATCH (u:User) WHERE u.age > 25 RETURN u
```

**Property Index:**
```cypher
// Create custom index
CREATE INDEX ON :User(email)

// Fast lookup
MATCH (u:User {email: 'alice@example.com'}) RETURN u
```

### Transactions

ACID transactions with optimistic concurrency control:

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
graph = db.graph("my_graph")

# Transaction automatically committed
with graph.transaction() as tx:
    alice = tx.create_node(['User'], {'name': 'Alice'})
    bob = tx.create_node(['User'], {'name': 'Bob'})
    tx.create_relationship(alice, 'KNOWS', bob)
```

## Storage Format

### Node Storage

```
NodeId: u64
├── Labels: Vec<String>
├── Properties: HashMap<String, PropertyValue>
├── Created: Timestamp
└── Updated: Timestamp
```

### Relationship Storage

```
RelationshipId: u64
├── Type: String
├── Start Node: NodeId
├── End Node: NodeId
├── Properties: HashMap<String, PropertyValue>
├── Created: Timestamp
└── Updated: Timestamp
```

### Property Storage

Properties are stored using efficient serialization:

```rust
pub enum PropertyValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<PropertyValue>),
}
```

## Performance Optimizations

### Bloom Filters

Fast membership testing reduces disk I/O:

```
Check if node exists:
1. Bloom filter (in-memory) → probably exists
2. RocksDB lookup → confirmed
```

### Block Cache

Configurable block cache for frequently accessed data:

```toml
[storage]
block_cache_size = "2GB"  # Default: 512MB
```

### Write Batching

Batched writes improve throughput:

```python
# Batch operations
with graph.transaction() as tx:
    for i in range(1000):
        tx.create_node(['User'], {'id': i})
# All committed together
```

### Compression

LZ4 compression balances speed and size:

```toml
[storage]
compression = "lz4"  # Options: none, snappy, lz4, zstd
```

## Configuration

Storage engine configuration in `config.toml`:

```toml
[storage]
path = "/data/qilbeedb"
block_cache_size = "512MB"
compression = "lz4"
max_open_files = 1000

[storage.rocksdb]
max_background_jobs = 4
write_buffer_size = "64MB"
max_write_buffer_number = 3
```

## Best Practices

1. **Index Frequently Queried Properties**
   - Create indexes for properties used in WHERE clauses

2. **Batch Large Writes**
   - Use transactions for bulk operations

3. **Configure Cache Size**
   - Set block_cache_size to ~25% of available RAM

4. **Regular Backups**
   - Schedule snapshot backups

## Next Steps

- Explore [Query Engine](query-engine.md)
- Learn about [Memory Engine](memory-engine.md)
- Configure [Performance Tuning](../operations/performance.md)
