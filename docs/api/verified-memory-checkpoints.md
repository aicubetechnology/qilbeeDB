# Verified consumer checkpoints

Available in **0.9.0**. Save progress through the [verified memory feed](verified-memory-changes.md)
using an exact history-bound cursor and a compare-and-set operation tied to the
previous checkpoint's revision **and digest**. Both are needed: after restore and
divergent writes, the same revision number can describe different progress.

## Permissions and ownership

All routes require `memory_read` and `memory_checkpoint` for the exact scope.
Ownership includes the credential's tenant and subject, resource scope, and your
`consumer_id`. Credential rotation preserves ownership; revocation stops access
immediately on subsequent requests. Another credential for the same subject and
scope can coordinate that consumer. A different subject cannot read or replace it.

`consumer_id` is 1–128 UTF-8 bytes and an idempotency key is 1–256 bytes. They must
not be blank or contain control characters. Credentials, company and private
subject identifiers are never taken from a checkpoint body.

## Read current progress

Call `POST /api/v2/memory/checkpoints/read`:

```json
{
  "contract_version": 2,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "consumer_id": "search-cache"
}
```

A successful response contains `contract_version: 2`, `scope` and `checkpoint`.
The checkpoint includes `schema_version: 2`, `consumer_id`, `revision`, the complete
verified `cursor`, authenticated `author`, `updated_at_millis` and
`checkpoint_digest`. A missing v2 checkpoint returns 404. The server checks stored
integrity and cursor history using the same read snapshot.

## Commit progress

Call `POST /api/v2/memory/checkpoints` with `scope` and a `command` containing:

| Field | Meaning |
| --- | --- |
| `contract_version` | Must be 2 |
| `idempotency_key` | Stable key for this exact command |
| `consumer_id` | Stable consumer name owned by the current subject |
| `expected_revision` | Zero to create; otherwise the current checkpoint revision |
| `expected_checkpoint_digest` | Null or omitted when creating; the exact current checkpoint digest when updating |
| `cursor` | Complete cursor copied from the v2 feed or activation response |

For an existing checkpoint, read its current state and copy **both** expectation
fields. A stale revision or digest returns 409 without changing progress. A new
checkpoint's expected digest must be null; an update requires 64 lowercase
hexadecimal characters. Do not generate or edit cursor digests yourself.

The target cursor must belong to the current verified history, including its
baseline and generation. Normal commits cannot move backward or switch histories.
Equal cursors are allowed and still advance the checkpoint revision. Commit only
after applying effects durably in your consumer. Checkpoint writes do not create
memory change events, modify records or acknowledge external transactions.

The response contains `contract_version: 2` and an immutable `receipt`, which
includes the idempotency key, original checkpoint and `receipt_digest`. Checkpoint
state and receipt commit together with synchronous WAL durability. Concurrent
commands using the same expectation have one winner; losers reread current state.

## Retries and recovery

Retry the **identical** command with the same key. You receive its original
receipt, even after progress advances or the credential rotates. This historical
receipt is not a current-state read and must not overwrite a newer client cursor.
A changed command under an existing key returns 409. Always read current progress
when resuming a consumer after uncertainty.

A cursor from writes lost by restore is rejected even after new writes catch up
with its sequence. A checkpoint comparison from another restore branch is also
rejected when its revision number matches but its digest differs. Reconcile the
consumer's effects with the restored memory state; never fabricate a new digest
or silently reinterpret a rejected cursor.

## Migrate from version 1

The [v1 checkpoints](memory-checkpoints.md) remain available in a separate durable
namespace. Creating or updating v2 progress does not alter them. A v1 cursor has
no generation or prefix anchor and cannot be promoted by guessing those values.

Activate the verified journal, obtain its current watermark, reconcile authorized
records and replay changes after that watermark. Only then create v2 progress
for your consumer with revision zero and a verified cursor. Coordinate which
consumer contract your application uses; parallel v1 and v2 checkpoints do not
coordinate each other's progress.

## Errors and limits

Malformed identities, expectations or cursor versions return 400; authentication
and authorization failures return 401/403. Missing current progress returns 404.
Conflicts, incompatible histories and changed retry bodies return 409. Stored
corruption returns 500 with no partial write. Standard 64 KiB request limits and
`Cache-Control: no-store` apply.

Progress and receipts are retained without automatic pruning. There are no
consumer leases, worker assignment, external exactly-once effects or automatic
rollback of work already performed outside QilbeeDB. Preserve backups and follow
the verified journal's upgrade and downgrade restrictions.
