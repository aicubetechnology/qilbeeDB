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
- `enable_wal` and `sync_wal` apply to unmanaged low-level entity batches.
  Synchronous WAL writes are enabled when both options are true. The unreleased
  [durable graph lifecycle](durable-graph-lifecycle.md) always synchronizes the WAL
  for managed named graphs, including when raw storage options disable it.

Entity formats and existing public method signatures are unchanged. These changes
apply to Rust storage transactions; a sequence of separate HTTP requests does
not become a transaction. Durable allocation metadata now participates in entity
batches. Graph catalog creation and retirement use separate synchronous batches;
arbitrary metadata writes are not part of an entity transaction.

## Isolation and recovery limits

Atomic publication is not serializable isolation. Transaction reads cache each
entity independently and do not share a database snapshot. Concurrent readers
making separate calls can observe different committed versions. Transactions
do not detect stale reads or conflicting updates: later writes can overwrite
earlier decisions. The shared writer mutex serializes publication, not the
application's entire read/modify/write sequence.

Low-level transactions do not add endpoint existence, uniqueness or cascade constraints.
The managed graph methods separately guard endpoint checks and detach deletion
under the shared writer lock, as documented in the lifecycle contract.
It does not rebuild stale index keys left by older versions. Property indexing
retains its existing hash format, including the unresolved ordering problem for
map-valued properties; cleanup guarantees for such values require a canonical
hash format and migration. Do not infer full index conformance from scalar
property regression tests.

The original atomic commit tests exercise preparation failure, concurrent writers,
database snapshots and graceful reopen. The lifecycle tests additionally kill a
process after acknowledged managed writes and verify recovery. Neither suite
simulates power loss, disk exhaustion, termination at every point during a write
or corrupted WAL recovery.

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
