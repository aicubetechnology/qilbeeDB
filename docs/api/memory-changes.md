# Memory change feed

Available in **0.8.0**. Use this feed to refresh scoped caches, reconnect agents and
observe committed memory mutations. It shares the existing durable memory store
and does not require a messaging service, model provider or background worker.
Check the deployed version with `/health`.

## Read committed changes

Call `POST /api/v1/memory/changes` with `Authorization: Bearer <credential>` and
`memory_read` for the exact scope. Tenant and private subject come from the current
credential, including on continuations. A cursor grants no additional access.

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "query": {"after": null, "through": null, "limit": 100}
}
```

The `page` response contains:

| Field | Contract |
| --- | --- |
| `changes` | At most `limit` journal entries in ascending sequence order |
| `next_cursor` | Exclusive starting point for the next request, or null before journal activation |
| `high_watermark` | Inclusive fence used by this request, or null before activation |
| `complete` | This request reached its fence; later writes may still arrive |

Each cursor has an opaque `journal_id` and unsigned 64-bit `sequence`. Ordering is
local to one authorized namespace. There is no global order across tenants,
projects, missions, agents or private subjects. `limit` must be 1–256; the standard
64 KiB request limit and `Cache-Control: no-store` apply.

Each entry preserves `schema_version`, `cursor`, `kind`, `record_id`,
`record_revision`, authenticated `author`, `committed_at_millis` and an opaque
`change_digest`. Kinds are `created`, `updated`, `deleted` and
`embedding_attached`, plus `reviewed` for [review decisions](memory-review.md). An embedding entry identifies the bound memory revision;
it does not increment that revision. Memory content, vectors and provider secrets
are never copied to the journal. Refetch an authorized record when needed.

A memory mutation, its idempotency receipt, integrity index and journal entry
commit in one synchronous WAL batch. A new embedding binding and its event also
commit together. An identical command retry creates no second event. Repeating
an identical existing embedding under a new idempotency key records its normal
receipt but adds no change event because the binding did not change. Conflicted
or rejected commands do not advance the journal.

## Catch up and then poll

1. Read a page with `through: null` and your last durable `after` cursor.
2. Retain that page's `high_watermark`. Pass it as `through` on subsequent pages
   and advance `after` to each returned `next_cursor`.
3. Apply entries idempotently using `(journal_id, sequence)` as the delivery ID.
   Save progress only after your local effects are durable.
4. When `complete` is true, retain `next_cursor`. For a new polling cycle, clear
   `through` so the next request can observe later commits.

The fence prevents a busy writer from moving the end of a catch-up cycle. The
service retains no pagination session; persisted cursors survive restart.
An empty page with `complete: true` means caught up to that fence, not that the
stream is permanently closed. A disconnected client can resume without replaying
already acknowledged sequence numbers. Retrying a read may redeliver entries;
this is not a guarantee of exactly-once external effects.

Fetch current record state when processing an entry; the record may have advanced
beyond `record_revision` or become unavailable. Never replace newer cached state
with an older event's inferred state. A deletion event remains readable as
metadata after its payload is unavailable. Treat any scoped change as invalidating
a cached ranking whose dependency set is unknown.

## Initial reconciliation and upgrades

The journal covers mutations committed by the platform API **after this feature
is active**. Existing records and historical idempotency receipts are preserved,
but the upgrade does not invent earlier journal entries. An old command retry
still returns its original receipt without creating synthetic history.

For a new consumer, first capture the current journal watermark, then enumerate
current authorized records with the [memory query API](versioned-memory.md).
After that scan, consume changes after the captured watermark and refetch current
state for affected IDs. If the initial watermark is null, start the change feed
with `after: null`. Reconcile repeated IDs by their revision and availability.
This protocol supports eventual cache reconciliation under concurrent writes; the
ordinary query scan is not a globally frozen database snapshot.

Expiry is clock-driven and creates no journal entry. Respect each record's
`valid_until_millis` and revalidate current access before using cached content.
Credential revocation is enforced on requests; it does not erase copies already
held by a client. The feed is not a content-erasure acknowledgement.

## Errors and retention

Malformed bounds or an `after` sequence beyond `through` return 400. A cursor from
another namespace/journal or ahead of restored state returns 409; do not silently
reset it to zero. A missing/invalid credential returns 401, missing capability or
scope grant returns 403, and inconsistent journal state or a missing expected
entry returns 500. No partial page is returned on a detected inconsistency.

Version 0.8.0 performs no automatic journal pruning and declares no cursor expiry.
Journal metadata therefore grows with mutations. Operators must retain its data
with the memory volume. Restoring an older backup may make a client cursor ahead
of the restored journal; reconcile that recovery explicitly. Configurable
retention, replication and cross-region ordering are separate capabilities.
