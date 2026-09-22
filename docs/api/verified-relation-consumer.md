# Consume relation changes with a lightweight Python client

**Source SDK preview; not yet merged or published to PyPI.**
`VerifiedRelationConsumer` uses the [typed relation feed](typed-relation-changes.md)
available in server 0.13.0. It validates a complete bounded page before delivering
any event, applies effects through a caller-owned durable sink, and advances an
owned checkpoint only after the sink retains the corresponding witness.

The client uses the Python standard library. It generates no embeddings, runs no
model, and embeds no QilbeeDB engine. The SQLite example is an optional destination
adapter. An existing Python package installation does not establish that this
source-preview client is available.

## Configure a stream-specific destination

The credential needs `memory_read` and `memory_checkpoint` in the exact scope.
Supply the expected company and subject explicitly. Each operation pins one
credential and checks its current identity before accessing the stream. A supplier
can return a rotated key on the next operation, with the same company and subject.

```python
import os
from qilbeedb import VerifiedRelationConsumer

consumer = VerifiedRelationConsumer(
    "https://api.example.com",
    lambda: os.environ["QILBEE_API_KEY"],
    tenant_id="company",
    subject_id="graph-cache-service",
    scope={
        "project_id": "project",
        "agent_id": "agent",
        "mission_id": None,
        "visibility": "private",
    },
    consumer_id="context-graph-cache",
    page_size=100,
    timeout=10.0,
    max_response_bytes=4 * 1024 * 1024,
)
```

`page_size` is 1–256. TLS verification, response byte limits, redirect rejection,
exact integer decoding, duplicate-field rejection and failure classification use
the same transport as the [memory client](verified-memory-consumer.md).
Credentials are not persisted, proxy environment variables are ignored, and no
request is retried automatically. The timeout bounds individual socket operations;
it is not a total call deadline.

A relation binding includes the stream, company, subject, exact scope and consumer
name. It differs from the memory client's binding even if all configured names
match. Server relocation or credential rotation does not change the binding.
Independent database lineages need separate sink namespaces; a binding is not a
server identity. Complete cursors and server diagnostics detect incompatible
history before incremental processing.

## Implement the durable boundary

| Object or method | Contract |
| --- | --- |
| `RelationCursor` | Complete immutable `version`, `stream`, `journal_id`, `sequence`, and `prefix_digest`; use `from_dict()` and `to_dict()` for exact serialization |
| `RelationDelivery` | Binding, stable delivery ID, cursor, and historical relation-change metadata |
| `load_witness(binding_id)` | Return the complete relation cursor for the highest contiguous prefix represented durably by this destination, or `None` before reconciliation |
| `apply(delivery)` | Commit idempotent effects, delivery deduplication, and the witness together before returning |

The client rejects memory cursors, nonconsecutive deliveries, inconsistent scope,
unsupported event kinds, invalid endpoint references and malformed digests. Python
integers preserve all unsigned 64-bit revision and sequence values. Digests are
opaque history witnesses verified by the server; the SDK does not independently
reconstruct the journal or claim to verify cryptographic signatures.

Commit the destination transaction before returning from `apply`. A callback that
fails after committing may leave durable effects; resumption uses that witness
instead of replaying already witnessed events. A callback that fails to retain
its cursor is rejected. Callbacks must be synchronous.

The SDK prevents reentry on one instance, but provides no distributed lease.
Coordinate independent sink writers and deduplicate inside their shared durable
boundary. Checkpoint comparison prevents a losing writer from replacing remote
progress, but cannot undo duplicate calls to an external service. Exactly-once
external effects require the destination's own idempotency and reconciliation.

## Initialize after explicit reconciliation

`diagnose()` observes the journal, current checkpoint and optional witness without
altering them. An inactive stream does not prove there are no preexisting
relations. A writer can explicitly
[activate the feed](typed-relation-changes.md#establish-an-initial-baseline), or a
new relation mutation activates it atomically.

For a new cache, invalidate or reconcile its existing authorized contents and
durably retain a verified relation cursor before calling `initialize`. Include
memory changes in this strategy. Neither feed represents an atomic snapshot across
both streams. Never seed the sink by copying a server checkpoint whose effects
the destination cannot demonstrate.

```python
from qilbeedb import RelationCursor

observation = consumer.diagnose()
if not observation["active"]:
    raise RuntimeError("Explicit journal activation and reconciliation are required")

cursor = RelationCursor.from_dict(observation["high_watermark"])
# Reconcile or invalidate the destination's graph cache and durably retain
# this exact cursor for consumer.binding_id. The application supplies `sink`.
# This comment is not a reconciliation step.
checkpoint = consumer.initialize(sink, cursor)
```

Initialization requires the exact existing sink witness and a missing server
checkpoint. It never replaces progress, rewinds a witness, or silently recovers
incompatible history. If a checkpoint already exists, resume through
`consume_once` after checking destination state.

## Catch up with bounded work

```python
fence = None
for _ in range(32):  # Application-owned work and cancellation budget.
    result = consumer.consume_once(sink, through=fence)
    fence = result.high_watermark
    if result.complete:
        break
```

Each call delivers at most one validated page. Retaining `high_watermark` as
`through` fixes a finite catch-up fence; later commits remain for the next cycle.
Retain that fence if the work budget ends before completion. Clear it only when
starting a new cycle. A checkpoint behind the sink witness can be advanced without
redelivering durable effects. A checkpoint ahead of the witness stops processing.

`RelationConsumptionResult` contains the delivery count, page cursor, high
watermark, completeness, freshly observed checkpoint, and optional historical
receipt. The current checkpoint can be newer than the page when another
coordinated worker advances it. A successful write is followed by current-state
diagnostics; an old receipt never substitutes for current progress.

## Queue invalidations safely with SQLite

`sdks/python/examples/verified_relation_sink.py` provides
`SQLiteRelationInvalidationSink`. Run source examples with
`PYTHONPATH=sdks/python:sdks/python/examples`. It stores no memory bodies, vectors
or credentials. A `synchronous=FULL` SQLite WAL transaction records each delivery,
advances the exact witness and marks the binding's graph cache dirty.

After explicit initial reconciliation, call
`seed_after_reconciliation(binding_id, cursor)` once. Existing witnesses cannot be
overwritten. `pending_invalidation(binding_id)` returns the latest dirty cursor.
After durably invalidating that cache, call
`acknowledge_invalidation(binding_id, observed_cursor)`. It clears only that exact
observation; a concurrently delivered newer event remains pending. Clearing an
invalidation never rewinds the sink witness or server checkpoint.

The example invalidates a whole binding. A selective destination needs a complete
reverse dependency map, including multi-hop cached results. Invalidating only an
assertion's two endpoint records can leave derived graph contexts stale. External
cache invalidation and queue acknowledgement are not a distributed transaction:
apply idempotent invalidation before acknowledging.

## Preserve current context across both streams

A relation event records a historical assertion operation, not current truth or
permission. Consume the [memory feed](verified-memory-consumer.md) as a separate
binding, too. Updating, deleting or rejecting a source can invalidate relations
without a relation event. Time-based expiry can occur without either stream
emitting an event.

Before reusing graph context, read current authorized
[typed graphs](typed-memory-graph.md) and/or relation eligibility, verify exact
endpoint revisions, and honor coverage and validity limits. On servers supporting
the 0.14.0 [additional context extension](typed-memory-relations.md#bind-the-context-used-for-inference),
include declared evidence sources and their transitive ancestors in cache
dependencies. A relation change event does not contain that full dependency set.
Use current relation reads, or conservatively invalidate the scoped cache after
a memory change. Treat 401/403, missing
relations, changed revisions and expiry as reasons to invalidate or reconcile.
Receiving all events does not grant a freshness lease. This SDK acknowledges
notifications; it does not materialize a current knowledge snapshot.

## Resolve failures without overwriting progress

Shared `ConsumerError` types expose `code`, HTTP `status` where available, and
`checkpoint_outcome`: `not_attempted`, `rejected`, `unknown`, or `acknowledged`.
These outcomes describe the checkpoint attempt, not whether sink effects happened.

| Condition | Response |
| --- | --- |
| Missing witness or checkpoint | Reconcile explicitly; do not infer completed effects |
| HTTP 409 comparison failure | Read current state; preserve the original sink witness |
| Lost response or malformed receipt | Treat the checkpoint outcome as unknown; resume by diagnosis rather than blind write retry |
| Failure after a valid receipt | The write was acknowledged, but current progress remains unverified |
| Divergent history or checkpoint ahead of the sink | Stop incremental effects and reconcile with retained evidence |
| Revocation, lost scope or identity mismatch | Resolve current authority; do not substitute another subject |
| Sink failure or malformed page | Stop; retain the last durable state and inspect the failure |

The SDK performs no `reconcile` checkpoint operation. Intentional history
replacement uses the server's evidence-bearing recovery contract after the
application reconciles its real effects. A jointly restored database and sink
cannot prove effects that both restored copies forgot. These guarantees concern
consumption integrity, not improved retrieval relevance or agent reasoning.
