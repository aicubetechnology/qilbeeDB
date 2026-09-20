# Atomic graph commits

`qilbee-storage` commits the final node and relationship state and its index
mutations in one RocksDB write batch. Previously, `Transaction::commit` applied
each operation separately: a later failure could leave earlier writes visible.

## Write contract

- A transaction's last operation on an entity determines its final state.
  Intermediate versions are never written to storage.
- Existing records are read and all new values are serialized before the batch
  is submitted. A preparation error publishes none of the transaction.
- Replacing a node removes the previous label and property index keys before
  adding its new keys. Replacing a relationship removes both previous adjacency
  entries, including when its type or endpoints change.
- Direct entity puts and deletes use the same batch preparation path.
- Cloned engine handles share a writer mutex across old-record reads and batch
  publication. Concurrent replacements cannot race while maintaining indexes.
- `enable_wal` and `sync_wal` apply to entity batches. Synchronous WAL writes are
  enabled only when both options are true. Disabling WAL sacrifices recovery of
  unflushed writes; the database's default profile does not enable synchronous
  WAL writes.

The storage format and public method signatures are unchanged. These changes
apply to Rust storage transactions; a sequence of separate HTTP requests does
not become a transaction. The graph metadata methods are outside this batch.

## Isolation and recovery limits

Atomic publication is not serializable isolation. Transaction reads cache each
entity independently and do not share a database snapshot. Concurrent readers
making separate calls can observe different committed versions. Transactions
do not detect stale reads or conflicting updates: later writes can overwrite
earlier decisions. The shared writer mutex serializes publication, not the
application's entire read/modify/write sequence.

This feature does not add endpoint existence, uniqueness or cascade constraints.
It does not rebuild stale index keys left by older versions. Property indexing
retains its existing hash format, including the unresolved ordering problem for
map-valued properties; cleanup guarantees for such values require a canonical
hash format and migration. Do not infer full index conformance from scalar
property regression tests.

The tests exercise preparation failure, concurrent writers, database snapshots
and graceful reopen. They do not simulate power loss, disk exhaustion, process
termination during a write or corrupted WAL recovery. Recovery guarantees need
a separately declared fault model and corresponding tests.

## Validation

Four reproductions failed before implementation: a partial commit after a
corrupt-record error, stale indexes after repeated node operations, stale indexes
after a direct replacement, and stale relationship adjacency entries.

Seven regression tests now cover those cases, concurrent cloned writers, paired
node visibility through RocksDB snapshots and entity/index recovery after a
synchronous-WAL commit and graceful reopen. The snapshot test uses RocksDB's
internal snapshot API; it does not introduce a public snapshot transaction API.

```bash
cargo test -p qilbee-storage atomic_ --locked
cargo test --workspace --all-targets --locked
```

See the [research roadmap](../research/agent-memory-evolution.md) for isolation,
fault injection and production readiness work still required.
