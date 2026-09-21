# Diagnose memory consumers

Available in the **0.10.0 feature preview** when the server exposes
`POST /api/v2/memory/consumers/diagnose`. Check the served OpenAPI: earlier 0.10.0
preview images do not contain this route.

Use this read-only endpoint to inspect a consumer's saved progress, the current
verified journal boundaries and an optional cursor retained by your application.
Every observation comes from one storage snapshot. The operation does not create
checkpoints, activate journals, acknowledge events or repair external state.

## Permissions and ownership

Supply a current bearer credential with both `memory_read` and
`memory_checkpoint` for the exact project, mission, agent and visibility scope.
The credential supplies the tenant and subject; the request cannot select another
owner. A different subject can read a shared journal if authorized, but cannot
inspect your checkpoint. Private journal boundaries are subject-specific too.
Rotation to another credential for the same subject preserves ownership.
Revocation applies to subsequent requests.

The endpoint inspects one `consumer_id`, not an unbounded registry of consumers.
Its identifier must be 1–128 UTF-8 bytes, nonblank and free of control characters.
Standard 64 KiB request limits and `Cache-Control: no-store` apply. Keep unsigned
64-bit sequences and distances lossless when using JavaScript clients.

## Request current state

```bash
curl --fail-with-body http://localhost:7474/api/v2/memory/consumers/diagnose \
  -H "Authorization: Bearer $QILBEE_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"contract_version":2,"scope":{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},"consumer_id":"search-cache","witness":null}'
```

`witness` is optional. To compare an earlier external observation, supply the
complete opaque v2 cursor previously returned by the verified feed or activation
endpoint. Copy all of its fields unchanged. Do not construct a cursor from a
sequence number or fabricate a digest.

A successful response contains `contract_version: 2`, the authorized `scope`,
and `diagnostics`:

| Field | Meaning |
| --- | --- |
| `consumer_id` | The requested subject-owned consumer |
| `active` | A verified journal exists in this scope |
| `baseline` | First resumable verified position, or null before activation |
| `high_watermark` | Current verified tip in this snapshot, or null before activation |
| `checkpoint` | Intrinsically validated stored v2 progress, or null if absent |
| `checkpoint_status` | `missing`, `compatible`, or `history_incompatible` |
| `pending_positions` | Tip sequence minus compatible checkpoint sequence; null when progress is missing or incompatible |
| `witness_status` | `not_provided`, `compatible`, or `history_incompatible` |
| `checkpoint_relative_to_witness` | `before`, `equal`, or `after` when both cursors match current history; otherwise null |

The checkpoint retains its revision, digest, cursor, authenticated author and
update time. An incompatibility does not erase it. No memory bodies, embeddings
or event list are returned. A malformed request or encountered storage failure
returns an error instead of a partial diagnostic.

## Interpret progress without hiding uncertainty

A missing checkpoint returns **200**, with `checkpoint_status: "missing"` and
`pending_positions: null`. It does not mean zero backlog, an empty corpus or a
new consumer. Check the selected scope and subject, then perform the required
initial reconciliation before initializing progress.

For compatible stored progress, `pending_positions` is a **sequence distance**.
It counts positions after the checkpoint up to the snapshot's tip, including
updates, reviews and embedding events. It is not a count of distinct memories,
currently eligible records or successfully audited events. Zero means the saved
cursor reaches the observed tip; it does not prove that external effects were
applied or that no new events arrived after the snapshot.

The checkpoint and witness statuses are independent. After a restore, the server
may contain a compatible older checkpoint while an externally retained witness
belongs to lost history. In that case the response can legitimately show
`checkpoint_status: "compatible"`, a numeric pending distance and
`witness_status: "history_incompatible"`. Stop incremental application and
reconcile the external consumer before trusting that distance as operational
progress. Do not silently replace the witness with the new tip.

If both positions are compatible, `checkpoint_relative_to_witness` compares
their sequences in the same verified history. `before` can result from effects
that were applied but not checkpointed, another worker's observation, intentional
recovery or restore; the comparison alone does not establish the cause.

An incompatible witness can also come from selecting a different scope or an
obsolete baseline. The response reports only the requested authorized scope; it
does not probe or identify the witness's source scope. If the application restores
its witness together with the database, the server cannot infer external effects
that both restored copies have forgotten. Preserve witnesses with the consumer's
durable effects when those effects must survive a database restore.

## Reconcile incompatible stored progress explicitly

A self-consistent checkpoint can refer to a history absent from a partially
restored database. Diagnostics returns it as `history_incompatible` with unknown
pending distance. Ordinary checkpoint reads and commits still fail closed for
that stored state. Diagnosis itself never changes it.

Starting with this feature, the existing
[explicit recovery operation](verified-memory-checkpoints.md#record-an-explicit-reconciliation)
can reconcile this case. Coordinate workers and durably reconcile external state;
then copy the exact `revision` and `checkpoint_digest` from the diagnostic,
choose a valid cursor from the current journal and submit an explicit recovery
command with a stable idempotency key and nonblank `evidence_ref`.

Recovery validates checkpoint ownership and integrity, both comparison fields and
the target history. A stale comparison returns 409 without replacing progress.
A target from lost history is also rejected. The successful recovery atomically
retains the previous checkpoint, replacement and caller-declared evidence in an
immutable receipt. Retrying that exact command returns its historical receipt;
it does not overwrite later progress. The evidence reference is recorded, not
fetched or externally verified. External effects are not rolled back by this API.

An invalid stored digest, broken journal boundary or missing encountered anchor
still produces `500 storage_inconsistency`. Recovery does not bypass those checks.
Investigate storage or restore a consistent backup; a different idempotency key
cannot repair corruption.

## Errors and work limits

| Status | Meaning and action |
| --- | --- |
| 200 | Inspect all statuses; success does not imply compatible history or completed effects |
| 400 | Invalid version, consumer identity, cursor encoding or request fields |
| 401 / 403 | Restore current authentication, both capabilities and the exact scope grant |
| 413 | Request exceeds the body limit |
| 500 | Encountered corrupt or inconsistent storage, or an internal failure; no partial diagnostic |

Well-formed incompatible cursors are observations in this endpoint's **200**
response. Unlike the verified feed, this diagnostic does not return 409 merely
because the witness is incompatible. Missing consumers likewise use a status
field, not 404. Common error envelopes retain `contract_version: 1`.

Application work is bounded by a fixed number of metadata and boundary lookups,
independent of the reported distance. The endpoint does not walk the middle of
the journal, enumerate all consumers or fetch memory payloads. Storage-engine
work and latency are not constant-time guarantees. To check intermediate events,
use the [bounded journal audit](verified-memory-changes.md#audit-a-bounded-journal-range)
and retain its explicit coverage. A successful diagnostic can coexist with
unencountered middle-history corruption; it is not a full-volume integrity claim.

The [lightweight Python consumer](verified-memory-consumer.md) uses this diagnostic
with a destination-owned durable witness before applying a bounded verified page.
It treats incompatible history as a reconciliation condition and does not reset
checkpoints automatically.
