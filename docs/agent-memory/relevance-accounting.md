# Native relevance accounting

Status: **available in the 0.14.0 Rust source**. This native library capability
does not introduce an HTTP endpoint. It does not change
HTTP cosine scores, hybrid ranking versions or procedural qualification.

## Purpose and ownership

Account elapsed time once, regardless of how often an application schedules
maintenance. Applications choose decay rates, access boosts, forgetting thresholds
and learning policies. The database preserves the accounting checkpoint and
serializes operations on the current record. A corrected score is not evidence of
improved retrieval or better agent task performance.

For a fixed rate `r` per hour over elapsed hours `t`, settlement multiplies the
current score by `exp(-r * t)`. Repeating settlement at the same timestamp applies
no further decay. Dividing an interval into smaller calls produces the same result
within floating-point precision when rate changes and access events are identical.

## Explicit Rust operations

Inside a fallible Rust function:

```rust
use qilbee_memory::relevance_accounting::AccountedRelevance;

let mut relevance = AccountedRelevance::new_at(0);
relevance.decay_at(0, 0.1)?;             // Activate application policy.
relevance.access_at(3_600_000, 0.05)?;   // Settle one hour, then apply the boost.
relevance.decay_at(7_200_000, 0.02)?;    // Settle the old rate, activate the new one.
```

The checkpoint records time, the active optional rate and whether accounting
started natively or adopted an older record. The first decay of a new native
record can charge its known interval at the supplied initial rate. Access before
any rate is active advances the checkpoint without inventing a rate. A later
rate change applies prospectively after settling the previous rate.

`access()` and `PersistentAgentMemory::get_episode` retain the historical 0.1
boost for compatibility. `apply_decay` retains the configured memory type's
existing rate. Use explicit operations when supplying a different application
policy. Access and decay now return a result: callers must handle invalid input
and backwards clock movement. Adding the checkpoint also changes Rust struct
literals; prefer constructors rather than manually creating incomplete state.

## Durable and concurrent use

`MemoryStorage::apply_relevance_change` applies an access or decay to the current
record while holding the backend mutation lock. It never writes back an old
caller-side copy. `invalidate_current_episode` similarly preserves committed
accesses and leaves an already invalidated record unchanged. Persistent agent
access, decay and invalidation use these operations. The RocksDB and in-memory
backends implement both, and the HTTP storage adapter forwards them through its
blocking worker. Custom backends default to an explicit unsupported error
until they implement equivalent atomic behavior.

Missing, foreign-scope or invalidated records are not updated by accounting.
Invalid rates or boosts, non-finite values and time earlier than the last recorded
event fail without changing that record. Access counts saturate at the maximum
unsigned 32-bit value, and the outcome reports saturation. Storage errors propagate;
normal WAL and synchronous-write configuration still determines durability.

These guarantees cover the atomic operations. General `store_episode` and
`update_episode` remain whole-record replacement APIs, not compare-and-swap. A
caller must not use a stale replacement to race these operations. A decay pass
over several records is not an all-or-nothing transaction: earlier successful
records remain updated if a later record fails. Automatic persistent-agent calls
use `AccessNow` and `DecayNow`, observing wall-clock time after acquiring the
backend lock. Explicitly timestamped changes retain their supplied time and reject
out-of-order events. A real backwards wall-clock adjustment can still be rejected.

## Older records and recovery

The reader accepts the original unversioned episode encoding and the QMEP v1
envelope through an explicit old-layout adapter. Their score, count, timestamps
and other fields are preserved; unknown prior accounting is represented explicitly.
The first accounting operation adopts such a record prospectively without
assuming whether an earlier process already decayed it. No historical rate is
inferred. An access can still apply the application's requested boost.

New writes use the QMEP v2 envelope, including the accounting checkpoint. This is
a storage-format transition: an older reader cannot safely consume v2 records.
Preserve a consistent pre-upgrade backup. Do not open a migrated directory with
an older binary or restore an old backup over acknowledged newer writes without
an explicit reconciliation procedure. Structured JSON content remains encoded
separately from the binary episode layout.

## Validation and remaining limits

Controlled-clock tests cover partitioned time, repeated timestamps, access and
rate changes, legacy adoption, invalid input and counter saturation. Original
old-format fixtures verify field preservation. Concurrent tests cover sixteen
accesses and races with invalidation in both built-in backends. A RocksDB reopen
test verifies that repeating a previously settled timestamp does not charge twice.

A child-process test kills the writer after a synchronous update is acknowledged,
then verifies recovery and same-timestamp replay. A real RocksDB read-only handle
rejects accounting and invalidation writes; neither change is persisted. These
checks do not simulate host power loss, every storage failure mode or durability
with WAL disabled. The workspace suite includes HTTP durability and schema
regressions. No benchmark improvement or production deployment is claimed by
this guide.
