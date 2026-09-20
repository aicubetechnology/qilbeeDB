# Verified memory change feed

Available in **0.9.0**. Use this contract when a consumer must recognize that a
restored database has diverged from the history it previously consumed. The
[version 1 feed](memory-changes.md) remains available with its original response
shape and sequence-only cursor semantics.

## Request a page

Call `POST /api/v2/memory/changes` with a current bearer credential granting
`memory_read` for the exact scope. The tenant and private subject are derived
from that credential on every request. Cursors never grant access.

```json
{
  "contract_version": 2,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "query": {"after": null, "through": null, "limit": 100}
}
```

The response has `contract_version: 2`, `scope` and `page`:

| Page field | Meaning |
| --- | --- |
| `active` | This scope has an anchored journal |
| `baseline` | First resumable position; older events are outside the verified suffix |
| `changes` | Up to `limit` entries, each with a verified `cursor` and the original `change` metadata |
| `next_cursor` | Exclusive starting position for the next page |
| `high_watermark` | Inclusive, fixed endpoint for this catch-up cycle |
| `complete` | This page reached that endpoint; later changes may still exist |

Before activation, `active` is false, cursors are null, `changes` is empty and
`complete` is true. This does **not** mean the scope contains no memories.
`limit` is required and must be 1–256. Standard 64 KiB request limits and
`Cache-Control: no-store` apply. No memory payload or vector is copied to the feed.

A verified cursor contains `version: 2`, `journal_id`, `generation`, unsigned
64-bit `sequence` and `prefix_digest` (64 lowercase hexadecimal characters).
Treat the complete object as opaque. Never construct it from a version 1 cursor,
edit its sequence or replace its digest. Use a lossless integer representation
for sequences above JavaScript's exact integer range.

## Resume with an exact history

1. Read with your durable `after` cursor and `through: null`.
2. Keep the returned `high_watermark` as `through` while paging. Advance `after`
   to each `next_cursor`, including on an empty completed page.
3. Apply changes idempotently. A delivery identity includes the complete verified
   cursor, so equal sequence numbers on divergent histories remain distinct.
4. Save progress only after the consumer's effects are durable. Clear `through`
   for the next polling cycle.

Each new mutation appends an anchor in the **same synchronous WAL batch** as its
legacy event, record/index changes and receipt. An anchor hashes the preceding
anchor, scope, generation, sequence, event digest and a new random commit nonce
using SHA-256 with a versioned domain separator. A retry returns the original
receipt without adding another anchor. Restart preserves anchors and fences.

A restored copy retains the anchors in its common prefix. Independent writes
produce distinct continuations, even if their event metadata happens to match.
The server rejects an `after` or `through` from a divergent suffix even after the
restored sequence catches up. A valid common-prefix cursor remains usable.

On 409, stop incremental application and reconcile the consumer's external state
with the restored database. Do not silently substitute the current watermark:
that would hide lost or divergent work. A database restore cannot undo actions
that a consumer already performed in another system.

## Upgrade boundary and initial reconciliation

The first new memory mutation after upgrading activates anchoring automatically.
You can also activate it explicitly before a migration or initial reconciliation.
For an existing journal, `baseline.sequence` is its previous tip: events at or
before that position are not retroactively verified. For a new journal the
baseline sequence is zero. Historical receipts and version 1 events are preserved.
An old command retry creates neither a new event nor a fabricated anchor.

A new consumer must enumerate current authorized records through the
[memory query API](versioned-memory.md), then replay changes after the watermark
captured before enumeration. Refetch current state and reconcile by revision and
availability. If anchoring is not yet active, enumerate and then start with
`after: null`; the first subsequent mutation supplies the baseline. This is an
eventual reconciliation protocol, not a multi-request frozen record snapshot.
Expiry is clock-driven and produces no event; honor record validity times and
current access independently.

## Errors and operational limits

| Status | Action |
| --- | --- |
| 400 | Correct contract version, cursor format, page limit or reversed bounds |
| 401 / 403 | Restore current authentication, capability or exact scope authorization |
| 409 | Reconcile: foreign generation/scope, position before the baseline, position ahead of the journal, or divergent prefix |
| 500 | Investigate missing/corrupt stored events or anchors; no partial page is returned |

Pages validate their cursors, current tip and the consecutive links they traverse.
They do not perform a full-volume integrity audit. Digests are not server signatures,
source-truth attestations, consensus or a defense against an administrator rewriting
all stored data. No journal pruning, external exactly-once effects or cross-region
replication is provided. Metadata storage grows with mutations.

After the first 0.9.0 write, use 0.9.0 or later for this volume. An older writer
cannot maintain the new anchor chain; 0.9.0 detects the resulting mismatch and
fails closed. To roll back the binary, restore the matching pre-upgrade backup
and explicitly reconcile consumers.

## Design and validation basis

[RocksDB checkpoints](https://github.com/facebook/rocksdb/wiki/Checkpoints) provide
consistent database copies used by the restore tests. The implementation tests a
shared prefix, divergent writes, sequence catch-up, fences, authorization,
corruption and restart. This is an application-level hash chain, not the Merkle
consistency-proof protocol defined by
[RFC 9162](https://www.rfc-editor.org/rfc/rfc9162.html).

## Activate a baseline explicitly

Call `POST /api/v2/memory/changes/activate` with **both** `memory_read` and
`memory_write` for the scope:

```json
{
  "contract_version": 2,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"}
}
```

The response contains `contract_version: 2`, `scope` and `baseline`. This operation
is naturally idempotent: concurrent calls, retries and later calls return the same
original baseline. No request identifier is required. It does not insert a memory,
advance the legacy sequence, backfill old events, reset a generation or establish
consumer progress. A read-only credential receives 403.

For an empty scope, activation durably establishes sequence zero. The next real
mutation extends that exact journal and generation. For an existing v1 journal,
the baseline is its current tip and the verified suffix starts with the next
mutation. Memories, receipts and legacy event bytes remain unchanged.

After activation, read the v2 feed and retain its **current high watermark** before
enumerating records. An activation retry returns the original baseline, which may
now be older than the current tip. Activation is a small synchronous metadata
transaction; it does not scan the corpus or generate embeddings. Restart before
the first memory mutation preserves the same baseline.

## Audit a bounded journal range

Available in **0.10.0**.

Call `POST /api/v2/memory/changes/audit` with the same request structure as the
verified feed and `memory_read` for the scope. It checks event integrity and
consecutive prefix links without returning event bodies, authors or record IDs.
The response contains `contract_version: 2`, `scope` and `audit`. For example:

```bash
curl --fail-with-body http://localhost:7474/api/v2/memory/changes/audit \
  -H "Authorization: Bearer $QILBEE_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"contract_version":2,"scope":{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},"query":{"after":null,"through":null,"limit":100}}'
```

Use a server running 0.10.0 or later. A 0.9.0 server does not expose this route.
This read-only operation requires only `memory_read`; it does not activate an
inactive journal or save consumer checkpoints. Responses use `Cache-Control: no-store`. Each page reauthorizes the current credential and exact scope,
including the authenticated subject for private memories.

| Audit field | Meaning |
| --- | --- |
| `active` | Whether anchoring is active in this scope |
| `baseline` | Earliest position covered by the verified contract |
| `checked_after` | Exclusive beginning of this checked page |
| `checked_through` | Inclusive end actually checked by this page; use as the next `after` |
| `high_watermark` | Fixed endpoint of the audit cycle; preserve as `through` |
| `links_checked` | Number of consecutive event/anchor pairs checked in this page, at most `limit` |
| `complete` | The checked page reached the requested fence |

For a complete audit of the anchored suffix, start with `after: null`, retain the
first `high_watermark`, then advance `after` using `checked_through` until complete.
Do not sum a retried page twice. If anchoring is inactive, the cursors are null,
`links_checked` is zero and no verified range has been audited. An active empty
journal returns its baseline, zero checked links and `complete: true`.

The limit is 1–256 pairs per request. Counts describe the selected range; they do
not count constant-size baseline, cursor and tip integrity checks. Each call uses
one storage snapshot and makes no writes. New mutations do not move a retained
fence. Restart preserves valid continuations; a divergent continuation returns
409 and requires reconciliation. A missing event or anchor, digest mismatch or
broken consecutive link returns 500 with no partial audit result for that page.

Checking only the current tip cannot establish that every middle entry is still
readable. This endpoint lets operators traverse that middle history with explicit
coverage and bounded requests. `complete` is **not** a full database health claim:
this audit does not verify pre-baseline legacy events, current record payloads or
indexes, embeddings, checkpoint history, authorization-store recovery, external
effects or the truth of memory content. Retain the exact scope, baseline, fence
and page results with your operational evidence; these responses are not signed
third-party attestations. No automatic auditor or retention policy is enabled.

### Handle audit results and failures

Save the first response's `high_watermark` as `through` for the entire cycle.
For each successful page, save `checked_after`, `checked_through` and
`links_checked`, then use `checked_through` as the next request's `after`.
Retain the same scope and stop when `complete` is true. On a network failure,
retry the same pair of cursors; account for that interval once. A new cycle may
choose a new fence. This does not create a multi-request storage snapshot: each
page independently validates its retained cursors against the current history.

| HTTP status | Meaning and action |
| --- | --- |
| `400` | Invalid version, limit, cursor encoding, or `after` later than `through`. Correct the request. |
| `401` / `403` | Credential is unavailable or lacks the required capability or exact scope. Resolve authorization before continuing. |
| `409` | A cursor does not belong to the current scoped journal history. Preserve the failed request and reconcile the restore boundary. |
| `500` | Storage or integrity failure. No partial successful audit is returned for this page. Preserve the previous successful boundary and investigate; do not treat a later tip check as proof that the failed range is healthy. |

The pair limit bounds work by event count, not elapsed time. An operator chooses
request pacing and the total range. The API does not silently repair, skip or
prune inconsistent entries.
