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

### Choose a recovery action

Starting in **0.10.0**, inspect `error.code` to choose the next action. Each row
is a failed operation: no partial checkpoint write or recovery is committed.

| HTTP status and code | Meaning | Client action |
| --- | --- | --- |
| `409 journal_history_conflict` | A well-formed target cursor does not belong to the current verified history, or the target scope has no verified journal. | Check the authorized scope and current baseline, then reconcile your consumer with current history. Do not invent a cursor or change a retry key to bypass this check. |
| `409 checkpoint_regression` | An ordinary checkpoint commit would move backward. | Read current progress. If an intentional rewind is required, coordinate workers and use explicit recovery with reconciliation evidence. |
| `409 revision_conflict` | Expected revision or digest is stale; recovery also uses this code when no current checkpoint exists. | Read current progress before deciding on a new command. A losing worker must not blindly overwrite the winner. |
| `409 idempotency_conflict` | A previously accepted key was reused with a different command body. | Retry the original command unchanged to obtain its receipt. Use a new key only for an intentionally new operation with current expectations. |
| `404 checkpoint_not_found` | No current progress exists for this subject and consumer in the exact authorized scope. | Verify subject and scope. Initialize progress only after the required initial reconciliation. |
| `404 recovery_not_found` | No recovery receipt exists at the requested resulting revision. | Check the subject, consumer and revision. Ordinary commits do not create recovery receipts. |

A code identifies the first failed check, not proof that every other field is
valid. After request validation, an existing retry receipt is checked before
current-state comparison. An identical retry returns its historical receipt;
a changed retry body remains an idempotency conflict. New commands compare
current progress before validating the target history and forward movement.
An invalid cursor encoding is rejected before testing whether its journal exists.

Error responses retain the common `contract_version: 1` envelope; successful v2
responses retain their existing contract version. A failed stored-checkpoint
integrity or history check is a server-side `500 storage_inconsistency`, not a
client-history conflict. Preserve that failure for investigation rather than
replacing stored progress. Malformed requests remain `400` and authorization
failures remain `401` or `403`.

In 0.9.0, history mismatches and backward commits used `idempotency_conflict`.
The new codes refine those existing `409` responses. Identical command retries,
comparison checks and durable state are unchanged. The OpenAPI now also includes
the existing checkpoint and recovery `404` codes. Refresh generated/cached client
contracts before upgrading, and retain a fallback for unfamiliar error codes.
These changes do not reinterpret version-one checkpoint conflicts.

Rust callers receive `Error::JournalHistoryConflict` or
`Error::CheckpointRegression` for these v2 client failures. Update exhaustive
matches on `qilbee_core::Error` when upgrading to 0.10.0. Both variants satisfy
`is_constraint_violation()`, and neither is classified as corruption or eligible
for recovery by retrying unchanged input through `is_recoverable()`.

Progress and receipts are retained without automatic pruning. There are no
consumer leases, worker assignment, external exactly-once effects or automatic
rollback of work already performed outside QilbeeDB. Preserve backups and follow
the verified journal's upgrade and downgrade restrictions.

## Record an explicit reconciliation

Use `POST /api/v2/memory/checkpoints/recover` when reconciling external consumer
state requires replacing existing progress, including moving backward. It requires
the same `memory_read` and `memory_checkpoint` capabilities and exact subject
ownership. Ordinary checkpoint commits continue to reject backward movement.

Coordinate your consumer workers, inspect the restored memory history and reconcile
their external state. Read the current checkpoint, choose a valid cursor from the
current verified feed, then submit `scope` and this recovery command:

| Field | Requirement |
| --- | --- |
| `contract_version` | 2 |
| `idempotency_key` | Stable recovery-operation key, 1–256 UTF-8 bytes |
| `consumer_id` | Existing subject-owned consumer, 1–128 UTF-8 bytes |
| `expected_revision` | Exact current checkpoint revision, at least 1 |
| `expected_checkpoint_digest` | Exact digest from the current checkpoint read |
| `cursor` | Complete currently valid v2 cursor; it may be earlier than current progress |
| `evidence_ref` | Nonblank reference to your reconciliation evidence, 1–2,048 UTF-8 bytes, no control characters |

The server does not fetch or verify `evidence_ref`. It records the caller's
reconciliation declaration, authenticated author, time, previous checkpoint,
replacement checkpoint and receipt digest. This does not verify the external
system, undo effects or acquire a worker lease. A cursor from a lost history
still returns 409; recovery is not an override of journal verification.

An existing checkpoint is required. A missing or stale expectation returns 409;
use an ordinary initial commit when no v2 checkpoint exists. A successful recovery
increments its revision and atomically stores the replacement, retry receipt and
immutable history. No memory event is emitted. Concurrent recovery or ordinary
progress writes using the same expectation cannot both win.

Retry the same recovery command with the same key to receive its original receipt.
A retry after later progress does **not** apply the old recovery again. Changing
its cursor, evidence, consumer or expectation under that key returns 409. Recovery
keys and ordinary commit keys are separate operation namespaces; applications
should still use descriptive unique keys to avoid confusion.

Read historical evidence through `POST /api/v2/memory/checkpoints/recoveries/read`:

```json
{
  "contract_version": 2,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "consumer_id": "search-cache",
  "revision": 2
}
```

`revision` identifies the checkpoint **produced by the recovery**, not its previous
revision. The response contains `contract_version: 2` and `receipt`, with
`previous`, `checkpoint`, `idempotency_key`, `evidence_ref` and `receipt_digest`.
A revision produced by an ordinary commit has no recovery receipt and returns 404.
History remains scoped to the owner subject, survives restart and enforces current
credential authorization. Use `/checkpoints/read` to obtain current progress.
