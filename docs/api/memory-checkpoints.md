# Durable consumer checkpoints

Available in **0.8.0**. Save an application's progress through the scoped memory
change feed in QilbeeDB so that it can reconnect from another process without
losing its acknowledged cursor. A checkpoint records progress; it does not execute
consumer work, acquire a worker lease or guarantee exactly-once external effects.

## Identity and authority

Both checkpoint routes require `memory_read` and `memory_checkpoint` for the
exact resource scope. A checkpoint belongs to the authenticated subject and a
caller-chosen `consumer_id` within that namespace. Even for shared memory,
different subjects have separate progress. Tenant administration alone does not
grant access. The request cannot select another subject or tenant.

Use a stable consumer ID of 1–128 UTF-8 bytes with no control characters or blank
value. Credential rotation within the same subject preserves ownership. A new
subject starts independent progress. Multiple processes using the same subject and
consumer ID share one checkpoint and must coordinate through its revision; they
do not acquire exclusive execution ownership by reading it.

## Read or create progress

Call `POST /api/v1/memory/checkpoints/read`:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "consumer_id": "context-cache"
}
```

The response is `{contract_version, scope, checkpoint}`. A never-created
checkpoint returns 404. Use the [change feed](memory-changes.md) to obtain the
cursor and complete initial reconciliation when consuming existing records.
During the initial memory query scan, set `filter.scan_limit` independently of
`filter.limit` to bound work even when many candidate records are unavailable.
An empty query page with `next_after` still requires continuation.

After applying a page's effects durably, call `POST /api/v1/memory/checkpoints`:

```json
{
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "command": {
    "contract_version": 1,
    "idempotency_key": "context-cache-page-1",
    "consumer_id": "context-cache",
    "expected_revision": 0,
    "cursor": {"journal_id": "00000000-0000-4000-8000-000000000042", "sequence": 100}
  }
}
```

Replace the example cursor with one actually returned by your authorized feed.
Use `expected_revision: 0` only to create missing progress. Subsequent commands
must supply the current checkpoint revision. A successful command increments that
revision and returns `{contract_version, receipt}`. `receipt` contains
`contract_version`, `idempotency_key`, `checkpoint` and `receipt_digest`.

A checkpoint contains:

| Field | Contract |
| --- | --- |
| `schema_version` | 1 |
| `consumer_id` | The subject-owned consumer identity |
| `revision` | Positive compare-and-set version; independent of event sequence |
| `cursor` | The acknowledged journal identity and sequence |
| `author` | Subject and credential that committed this version |
| `updated_at_millis` | Server commit time |
| `checkpoint_digest` | Namespace-bound integrity digest |

The cursor must belong to the current journal and cannot exceed its high
watermark. Sequence zero is accepted when that journal already exists; no
checkpoint can be created before journal activation. Existing progress cannot
move backward or switch journal identities. Writing the same cursor with a new
idempotency key and correct expected revision is allowed and advances the
checkpoint revision. A competing command for the old revision returns 409.

Checkpoint and receipt commit in one synchronous WAL batch in the memory store.
Checkpoint writes do not emit memory-change events and therefore cannot create a
feedback loop in the feed. They do not prune journal entries or establish a
retention entitlement.

## Retry and reconnect correctly

1. Read the current checkpoint. If absent, follow the feed's initial
   reconciliation procedure and start from the appropriate cursor.
2. Read the next change page using its cursor as the exclusive `after` value.
   Retain a catch-up `through` fence separately if your application needs one.
3. Apply each external effect idempotently using `(journal_id, sequence)` as its
   delivery identity. Commit those effects before acknowledging the page.
4. Save `next_cursor` with the current checkpoint revision and a new idempotency
   key. On a lost response, retry the identical command with that same key.
5. On a revision conflict, reread current progress and reconcile concurrent work.
   Do not overwrite another worker's newer progress.

An identical retry returns its original receipt, even after progress advanced.
That receipt may be older than the current checkpoint. Read current state before
using its revision for a new command. Reusing the same idempotency key for another
cursor, consumer or expected revision returns 409. Authentication, revocation and
current scope grants are checked on every call, including retries.

A crash after external work but before saving progress can redeliver that work.
A crash after saving progress is recovered from the acknowledged server cursor.
QilbeeDB cannot atomically commit a checkpoint with a separate application's
database or external side effect. The server also cannot establish that a caller
actually processed every event through the supplied sequence.

## Recovery and limits

Checkpoint reads observe one current stored value. Feed pages and checkpoint
writes are separate operations, and another writer may append changes between
them. Saving an earlier page cursor remains valid as long as it is monotonic and
within the journal range. An empty page can retain the current checkpoint.

Restoring an older backup can put the server behind external consumer effects or
client cursors. A cursor ahead of the restored journal or from another journal
returns 409. Reconcile those effects explicitly. If a replaced journal requires a
new starting point, use a new consumer identity after reconciliation; there is no
implicit reset, rollback or checkpoint deletion endpoint in this release.

Malformed identities or contracts return 400; invalid/revoked credentials return
401; missing capability or grant returns 403; absent progress returns 404. Stale
compare-and-set revisions, nonmonotonic/foreign/ahead cursors and idempotency
conflicts return 409. Detected integrity failures return 500. Standard 64 KiB
request limits and `Cache-Control: no-store` apply. Digests detect inconsistent
stored values; they are not signatures or evidence that external effects occurred.

Checkpoints and retry receipts are retained without automatic pruning. There are
no consumer leases, automatic retries, background workers or cross-region
replication implied by this contract. The local acceptance suite uses ten
independent consumer subjects, durable reconnects, concurrent checkpoint writes
and forced process termination; it is a correctness test, not a throughput or
high-availability qualification.
