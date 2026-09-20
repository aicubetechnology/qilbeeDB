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
