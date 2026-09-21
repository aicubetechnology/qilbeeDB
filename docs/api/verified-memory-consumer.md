# Consume memory changes with a lightweight Python client

The source SDK includes `VerifiedMemoryConsumer`, a synchronous, standard-library
client for the verified memory feed. It requires a server exposing
`/api/v2/memory/consumers/diagnose`, included in merged main 0.10.0 at
`e4308e1bcda13b87158ae6bef1fc023fe9cd787f`. Earlier 0.10.0 preview images do not
expose that route. This SDK is available from source; installing an existing
PyPI package does not establish that the consumer is available.

The client reads one bounded page, validates it before delivering any events,
applies durable effects through your sink, and commits progress with the exact
checkpoint revision and digest. It does not generate embeddings, require model
credentials, run an agent, or embed a storage engine. The application chooses its
own durable destination. Legacy graph SDK imports load only when requested.

Use the separate [relation consumer](verified-relation-consumer.md) for typed
assertion changes. The two clients share delivery safeguards but use distinct
bindings, cursor types, routes and checkpoint contracts. A graph cache needs both
streams and current eligibility checks; neither stream emits clock-driven expiry.

## Configure identity and scope

Use a current credential with `memory_read` and `memory_checkpoint` for the exact
scope. Configure the expected tenant and subject explicitly. Every operation
obtains one credential, checks `/api/v1/identity`, and uses that credential for the
whole operation. A supplier can return a rotated credential on the next call;
it must still belong to the configured tenant and subject. The server reauthorizes
each request, including after revocation.

```python
import os
from qilbeedb import VerifiedMemoryConsumer

consumer = VerifiedMemoryConsumer(
    "http://localhost:7474",
    lambda: os.environ["QILBEE_API_KEY"],
    tenant_id="company",
    subject_id="search-service",
    scope={
        "project_id": "project",
        "mission_id": None,
        "agent_id": "agent",
        "visibility": "shared",
    },
    consumer_id="search-cache",
    page_size=100,
    timeout=10.0,
    max_response_bytes=4 * 1024 * 1024,
)
```

The stable `binding_id` binds tenant, subject, exact scope and consumer name.
Credential rotation and moving the same database to another URL preserve it.
Distinct independent database lineages need distinct sink namespaces even when
these configured names match. The complete verified cursor detects incompatible
history; the binding alone is not a server identity or an authorization grant.

Use HTTPS outside a trusted local transport. The client verifies TLS with Python's
default trust configuration. It ignores proxy environment settings, does not
follow redirects, persists no credential, retries no request automatically, and
returns structured errors without copying server message bodies or bearer tokens
into exception text. `timeout` is a socket-operation timeout, not a total deadline
against a peer that continually sends bytes. Responses are capped after transport
decoding; compressed responses, duplicate JSON fields, duplicate framing headers,
nonfinite numbers and incompatible contracts are rejected.

## Define the durable sink boundary

Implement two synchronous methods:

| Method | Contract |
| --- | --- |
| `load_witness(binding_id)` | Return the complete `VerifiedCursor` for the highest contiguous prefix whose effects are durably represented in this destination, or `None` before reconciliation |
| `apply(delivery)` | Durably apply the event idempotently and retain its cursor before returning |

A `MemoryDelivery` contains `binding_id`, a stable `delivery_id`, its complete
verified `cursor`, and event `change` metadata. The delivery ID binds the full
cursor, including its prefix digest; equal sequence numbers on divergent histories
are not the same delivery. Preserve unsigned 64-bit integers without converting
them to floating point. The client does not recompute the server's cursor digests
or treat them as signatures.

Effects, deduplication and the witness must share the destination's durable commit
boundary. Never advance a witness before its effects are durable or skip an event. Coordinate
writers to each sink binding; the client prevents reentry on one instance but
provides no distributed lease. Independent workers may perform duplicate effects
before one loses the server checkpoint comparison. Nontransactional external
services require their own idempotency receipts and reconciliation policy.

The repository's `sdks/python/examples/verified_consumer_sink.py` provides a
`SQLiteInvalidationSink` example. It atomically records delivery deduplication,
record invalidation metadata and the witness in a local SQLite transaction. It
uses only the standard library and preserves large revision numbers as text.
SQLite is an example destination, not an SDK requirement. Run examples from the
checkout with `PYTHONPATH=sdks/python:sdks/python/examples`.

The example queues invalidations; it does not materialize a knowledge snapshot.
A separate worker must refetch current authorized memory, handle missing records
and revision changes, and expire cached data independently. The event feed contains
no record bodies or embeddings. Clock-driven expiry emits no event, and receiving
an event does not prove that its old revision is still available or authorized.

When the server advertises `/api/v1/memory/records/batch`, the application can
[reread a group](memory-batch-read.md) in one record/dependency snapshot and compare
every retained revision. The SDK does not automatically make that request or
provide a reusable context cache. Batch evaluation is not a freshness lease.

## Initialize only after reconciliation

A missing server checkpoint or sink witness stops consumption. Neither is treated
as proof that the scope is empty. The SDK never activates a journal or silently
starts at its tip.

1. Select the correct identity, scope and sink binding. Use the
   [activation API](verified-memory-changes.md#activate-a-baseline-explicitly)
   if the journal is inactive; activation needs a separate authorized writer.
2. Read the current verified feed watermark **before** enumerating current
   authorized memories. An activation retry returns the original baseline, which
   can be older than the current watermark.
3. Reconcile the destination, including absent, expired or inaccessible records.
   Persist the selected watermark as the destination's initial witness only after
   that reconciliation is durable. Follow the feed's eventual reconciliation
   protocol; enumeration is not a multi-request snapshot.
4. Call `initialize(sink, cursor)` with that exact complete cursor, then replay
   changes after it. Initialization uses expected revision zero and never
   overwrites an existing checkpoint.

For the example sink, the final steps are:

```python
from qilbeedb import VerifiedCursor
from verified_consumer_sink import SQLiteInvalidationSink

sink = SQLiteInvalidationSink("consumer.sqlite")
# retained_watermark is the complete server cursor captured before enumeration.
# Reconcile the destination durably before these two calls.
cursor = VerifiedCursor.from_dict(retained_watermark)
sink.seed_after_reconciliation(consumer.binding_id, cursor)
consumer.initialize(sink, cursor)
```

The example refuses to overwrite an existing witness. If initialization's response
is lost, keep that witness, inspect diagnostics and resume existing progress with
`consume_once`. If the checkpoint is still missing, repeat explicit initialization
with the same cursor. Never re-enumerate and replace progress merely because a
response was lost.

## Consume a bounded catch-up cycle

```python
fence = None
while True:
    result = consumer.consume_once(sink, through=fence)
    fence = result.high_watermark
    if result.complete:
        break
# A later polling cycle starts again with through=None.
```

Each call delivers at most `page_size` events, with a limit of 1–256. Keep the
returned `high_watermark` as `through` for a finite catch-up cycle. `complete`
means that page reached its fixed fence, not that no newer event exists. Cancel
or stop between calls; no background worker or automatic polling is created.

The sink witness can be ahead of the server checkpoint after a callback failure,
a process crash, or a missing commit response. When both remain compatible, the
SDK requests the next page **after the durable sink witness**. The server verifies
that witness in the same snapshot as the page, so a restore between diagnosis and
feed retrieval cannot bypass cursor validation. An empty page can still commit
previously durable effects that the server has not acknowledged.

Every complete page is structurally checked before the first callback: exact
scope and versions, allowed event kinds, consecutive positions, legacy/verified
cursor agreement, requested limit, fence, next cursor and completion. After each
callback, the SDK checks that the sink retained that event's exact cursor.
An exception, asynchronous callback, missing witness or malformed page stops the
call without attempting a new checkpoint. Earlier destination effects may already
be durable and remain recoverable through that witness.

The commit uses the original server checkpoint revision and digest, never a fresh
expectation obtained after losing a race. Its stable key binds the exact command
and consumer binding, allowing identical replay after a crash. A successful commit
is followed by a fresh diagnostic: `result.receipt` is historical evidence, whereas
`result.checkpoint` is the observed current state and can be newer than that receipt.
Neither represents an indefinitely current state or proves external exactly-once
effects. A failed follow-up read raises an error instead of substituting the receipt
as current progress.

## Handle failures without replacing progress

`ConsumerError` exposes `code`, optional HTTP `status`, and `checkpoint_outcome`.
Subclasses distinguish API, protocol and transport failures, and conditions that
require explicit reconciliation. Outcomes describe this call's checkpoint attempt;
they do not state whether destination effects happened.

| Outcome | Meaning |
| --- | --- |
| `not_attempted` | No checkpoint request was attempted by this operation |
| `rejected` | The attempted checkpoint received a structured HTTP 4xx rejection |
| `unknown` | The request may have committed; no trustworthy successful receipt was obtained |
| `acknowledged` | A valid successful receipt was received, but the required subsequent observation failed |

Preserve the sink and retry consumption by reading current server state. Do not
reset progress or reuse a changed command under an old key. A fresh call may
acknowledge already durable work with zero new callbacks. Callers own retry limits,
backoff, cancellation and operational monitoring; the SDK performs no hidden retry.

| Condition | Required action |
| --- | --- |
| `checkpoint_initialization_required` / `durable_witness_required` | Verify identity and destination; perform explicit initial reconciliation |
| `revision_conflict` | Another operation changed progress; inspect current state before a new attempt |
| `journal_history_conflict` / `history_reconciliation_required` | Stop incremental application and reconcile current history with destination effects |
| `checkpoint_ahead_of_durable_effects` | The sink cannot prove the server's acknowledged prefix; reconcile instead of skipping to it |
| `sink_apply_failed` / `sink_did_not_retain_delivery` | Repair the sink while retaining its last durable witness; prior effects may exist |
| `credential_identity_mismatch`, 401 or 403 | Resolve tenant/subject configuration, current credential and exact scope grants |
| Protocol error or 500 | Investigate the contract or storage failure; do not fall back to unverified v1 cursors |

Use [consumer diagnostics](consumer-diagnostics.md) to inspect incompatible
progress. Any intentional rewind or replacement remains an
[explicit recovery operation](verified-memory-checkpoints.md#record-an-explicit-reconciliation)
with exact comparisons and reconciliation evidence; this SDK does not automate it.
A sink witness restored together with the database cannot reveal effects that
both restored copies forgot. Coordinate backups and preserve the destination's
actual durability boundary. These guarantees concern consumption integrity, not
retrieval relevance, learning quality or downstream agent performance.
