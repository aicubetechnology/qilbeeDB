# Durable graph lifecycle

Status: **0.12.0 Rust library contract**. This guide covers named graphs in
`qilbee_graph::Database`. It does not add a graph endpoint to the platform HTTP
API or change memory search ranking. See the [graph memory evidence map](../research/graph-memory-evidence.md)
for the separate integration and evaluation work.

Named graphs preserve their identity and allocation state across restarts. A
successful creation, mutation or retirement is acknowledged after a synchronous
RocksDB write-ahead log (WAL) write. Reopening a graph continues its node and
relationship counters. Retiring a graph and later creating the same name produces
a new, empty graph generation.

## Open and retire a graph

```rust
use qilbee_graph::Database;

fn example() -> qilbee_core::Result<()> {
    let database = Database::open("./graph-data")?;
    let graph = database.graph("support-knowledge")?;
    let incident = graph.create_node(["Incident"])?;
    let procedure = graph.create_node(["Procedure"])?;
    graph.create_relationship(incident.id, "USES", procedure.id)?;

    let old_generation = graph.id();
    database.delete_graph("support-knowledge")?;
    assert!(graph.get_node(incident.id).is_err());

    let replacement = database.create_graph("support-knowledge")?;
    assert_ne!(replacement.id(), old_generation);
    assert!(replacement.get_all_nodes()?.is_empty());
    Ok(())
}
```

Graph names contain 1–1024 UTF-8 bytes, must contain a non-whitespace character
and cannot contain control characters. Names are case-sensitive and are not
trimmed or normalized. `graph(name)` returns the active generation or creates
one. `create_graph(name)` fails if that name is active. `list_graphs()` returns
active names in lexical order; `graph_exists()` and `graph_count()` read the
durable catalog rather than a wrapper's cached handles.

`delete_graph(name)` returns `true` when it retires an active generation and
`false` when the name is absent. It performs **logical retirement**: physical
records remain in storage and consume disk space. It is not secure erasure or a
retention cleanup operation. A trusted low-level storage reader can still inspect
those records by their old graph ID. No physical purge API is introduced here.

## Identity and allocation

Persist a graph ID together with each node or relationship ID when referring to
an entity outside its graph handle. Node IDs and relationship IDs are separate
unsigned 64-bit sequences within a graph generation. Their numeric values may
overlap and may start again in a different generation. Names are reusable;
generation IDs are reserved durably and never intentionally reused.

A candidate generation ID is checked against active and retired reservations,
allocation metadata and retained graph record/index prefixes. Randomness proposes
an ID; the reservation checks enforce uniqueness. An allocation failure returns
an error without publishing a partial graph.

High-water marks are written in the same batch as entity and index changes.
Deleting the highest entity does not lower its counter. A successful explicit
low-level put also advances the counter, even if a later operation deletes that
entity in the same transaction. Automatic allocation uses checked arithmetic;
an exhausted counter returns an error instead of wrapping around. Explicit
low-level puts remain replacements, not insert-only operations.

## Concurrency and transaction boundaries

Cloned storage engines share one mutation lock. Independent `Database` wrappers
over those clones use the same durable catalog and counters. Creation checks,
catalog capacity checks and publication happen under that lock. `max_graphs`
is the calling database's configuration, not a persisted tenant quota. Separate
processes cannot concurrently open the same RocksDB directory as writers.

Graph node validation and publication run under the writer lock. Relationship
creation and update verify both endpoints under that lock. Node deletion checks
incident relationships there as well; `detach_delete_node` removes the node and
its incident relationships in one batch. The lower-level storage validation
callbacks may read the engine but must not write to it recursively.

Handles obtained before retirement reject subsequent graph reads and mutations.
Transactions created by `Graph::begin_transaction` retain that generation and
recheck it when accessing cached records, buffering operations and committing.
They cannot commit into a retired or replacement generation. Local rollback
still works after retirement. Reads already started before retirement can finish;
retirement does not cancel an in-flight application computation.

Transactions retain the [atomic commit contract](atomic-commits.md): reads are
not a shared snapshot, and commits do not detect stale-read conflicts. Explicit
transaction operations do not provide the high-level graph's endpoint or schema
validation. Schema definitions remain in memory, are shared by clones of a graph
handle, and are neither durable nor shared by independently constructed database
wrappers. These limitations must be resolved before relying on schemas as a
database-wide integrity boundary.

`StorageEngine` and its raw metadata methods are trusted embedded interfaces.
A graph identity is a lifecycle guard, not an authentication credential or a
tenant authorization policy. HTTP authorization must be enforced separately.

## Upgrade existing graph data

The first catalog access migrates the legacy JSON list of graph names into a
versioned catalog, generation descriptors and allocation metadata in one
synchronous batch. Existing graphs retain their established name-derived graph
IDs and physical entity keys. The migration initializes counters from the highest
retained node and relationship keys, or preserves a valid existing higher counter.
It does not scan or rewrite every payload.

Duplicate legacy names, colliding legacy IDs, invalid names and inconsistent
metadata fail with an error before migration publication. Missing or malformed allocation metadata, or a counter lower than the highest
retained entity key, prevents managed entity writes. Missing
catalog metadata when generation descriptors exist also fails closed. These
checks do not constitute a complete audit of all data or indexes.

Before upgrading, stop writers and take a restorable backup of the entire
database directory using the [backup procedure](../operations/backup.md). Exercise
the upgrade and application reads on a restored copy first. The new catalog is
not readable by the old name-list implementation. Downgrade requires restoring
the pre-upgrade backup; do not edit the catalog back into a name list or expect
post-upgrade writes to be retained by a downgrade.

Legacy counters did not retain historical allocations. The migration cannot
reconstruct IDs whose records were already deleted before the upgrade, or recover
records already overwritten by the previous restart bug. It preserves retained
data and prevents further automatic reuse within the upgraded generation.

## Durability and operational cost

Managed graph catalog changes and entity writes always enable and synchronize
the WAL, even when `StorageOptions` disables WAL or asynchronous writes are
requested for raw storage. This is an intentional durability change and can
increase write latency. No throughput improvement has been established. Batch
related entity operations when their validation requirements permit it, and
measure latency with the intended disk, concurrency and batch sizes.

Unmanaged low-level graph IDs retain the configured WAL options. Do not infer the
managed recovery contract from a raw `StorageEngine` write to an arbitrary ID.
Successful synchronization relies on the operating system and storage device
honoring the write contract; this qualification does not simulate power loss,
torn disk writes, filesystem corruption or disk exhaustion.

## Validation

Three regressions failed before implementation: node overwrite after reopening,
relationship overwrite after reopening, and old records appearing when a retired
name was recreated. Tests now cover these failures, deleted high-water IDs,
stale handles and transactions, concurrent wrappers, capacity races, endpoint
deletion races, legacy migration, invalid metadata, ID exhaustion and all-or-nothing
batch preparation.

A subprocess test disables raw WAL options, acknowledges managed writes, then
is killed without closing the database. Reopening verifies entity recovery,
continued allocation above deleted IDs, durable retirement and an empty
replacement generation. This is a process-crash test after acknowledgement;
it does not test every possible crash point during migration or a write.

```bash
cargo test -p qilbee-storage -p qilbee-graph --locked
cargo test --workspace --all-targets --locked
```

These are storage correctness tests. They do not establish better retrieval,
agent reasoning, task completion or autonomous learning.
