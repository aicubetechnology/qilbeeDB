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
  Synchronous WAL writes are enabled when both options are true. The 0.12.0
  [durable graph lifecycle](durable-graph-lifecycle.md) always synchronizes the WAL
  for managed named graphs, including when raw storage options disable it.

Entity formats and existing public method signatures are unchanged. These changes
apply to Rust storage transactions; a sequence of separate HTTP requests does
not become a transaction. Durable allocation metadata now participates in entity
batches. Graph catalog creation and retirement use separate synchronous batches;
arbitrary metadata writes are not part of an entity transaction.

## Isolation and recovery limits

Transactions now validate every observed node and relationship before commit.
The first read or mutation captures the exact stored bytes, including absence.
Pending writes have a separate cache, so reading your own writes does not replace
the original observation. Under the shared writer mutex, commit validates graph
generation and every observation before publishing the atomic entity/index batch.
A changed observation returns `Error::TransactionAborted` and publishes nothing.
This also protects negative reads, read-only transactions and blind writes whose
stored state changes after the first mutation. Changes to unobserved entities do
not cause conflicts.

This is a behavior change for Rust callers: commits that previously silently
overwrote concurrent changes can fail. `Transaction::new`, `Transaction::for_graph`,
`StorageEngine::begin_transaction` and `Graph::begin_transaction` use this same
contract. Method signatures and on-disk formats are unchanged. This does not
change the platform HTTP memory revision contract or combine separate requests
into a transaction.

After a conflict, discard decisions derived from the rejected transaction. If
appropriate for the application, start a new transaction, read current state and
recompute the intended database change. The database does not retry business
logic or replay external effects. A conflict is not evidence that an external
operation failed or is safe to repeat.

Point-read validation does not provide a historical snapshot at transaction
start, predicate/range locking, or isolation for reads performed outside the
transaction interface. Separate reads may observe different versions before
commit; callers must not treat tentative values as committed decisions.
Byte-identical restoration is accepted: the guard checks current value equality,
not whether any intervening history exists. The shared writer mutex covers
validation and publication, not the application's entire lifetime.

Low-level transactions do not add endpoint existence, uniqueness or cascade constraints.
The managed graph methods separately guard endpoint checks and detach deletion
under the shared writer lock, as documented in the lifecycle contract.
Startup now reconstructs legacy property indexes using a canonical equality-compatible
format, including map and signed-zero values. See [property index upgrades](property-index.md)
for readiness, interruption recovery and mandatory rollback precautions. Transaction
atomicity alone does not repair arbitrary index corruption.

Process-restart recovery does not establish behavior under power loss, disk
exhaustion or corrupted WAL recovery. Validate those failure modes for your
storage configuration and retain an application-consistent backup. Internal
storage snapshots do not introduce a public snapshot transaction API.
