# Consume typed relation changes and retain progress

**Unreleased 0.13.0.** This contract delivers committed changes to
[typed memory relations](typed-memory-relations.md), with history-bound cursors
and durable, subject-owned consumer checkpoints. Use it to invalidate cached
[typed graphs](typed-memory-graph.md) or drive external consolidation workers.
It performs no model inference and does not establish a learning-quality gain.

The relation stream is separate from the existing memory feeds. Existing memory
clients keep their event kinds and cursor semantics. A graph cache must also
observe memory changes and revalidate eligibility before reuse: changing or
expiring an endpoint does not create a relation event.

## Choose the correct authority and route

All routes use `Authorization: Bearer <credential>`. Company and private subject
come from current authentication, including on retries and continuations. A
cursor, consumer ID or digest grants no access.

| POST route | Required capabilities | Purpose |
| --- | --- | --- |
| `/api/v1/memory/relations/changes` | `memory_read` | Read a bounded suffix in one authorized scope |
| `/api/v1/memory/relations/changes/activate` | `memory_read`, `memory_write` | Establish a baseline without fabricating old events |
| `/api/v1/memory/relations/checkpoints` | `memory_read`, `memory_checkpoint` | Advance or explicitly reconcile owned progress |
| `/api/v1/memory/relations/checkpoints/read` | `memory_read`, `memory_checkpoint` | Read current, history-compatible progress |
| `/api/v1/memory/relations/checkpoints/revision` | `memory_read`, `memory_checkpoint` | Read an immutable checkpoint receipt |
| `/api/v1/memory/relations/consumers/diagnose` | `memory_read`, `memory_checkpoint` | Compare current history, progress and a local witness |

Each route shares the server's retrieval admission slots through serialization.
Bodies are limited to 65,536 bytes and responses use `Cache-Control: no-store`.
These are scoped consumer routes; native company administration does not create
or impersonate another subject's checkpoint.

## Read a bounded change page

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "mission_id": null,
    "agent_id": "agent",
    "visibility": "private"
  },
  "query": {"after": null, "through": null, "limit": 100}
}
```

`limit` is required and must be 1–256. `after` is exclusive; `through` is an
inclusive fence. Omit them or send null when starting a new catch-up cycle.
The response contains `contract_version`, `scope` and `page`:

| Page field | Meaning |
| --- | --- |
| `active` | Whether this namespace has a relation journal |
| `baseline` | The complete zero-position cursor where this journal began |
| `changes` | At most `limit` consecutive deliveries, ordered by sequence |
| `next_cursor` | Complete cursor after the delivered prefix |
| `high_watermark` | The inclusive fence selected or verified in this request |
| `complete` | Whether this page reached its fence |

An inactive journal returns null cursors, no changes and `complete: true`.
That means no relation journal is active; it does not prove that the scope has
no retained relations. Supplying a cursor for an inactive stream returns 409.

Each delivery contains `cursor` and `change`. The cursor has `version: 1`,
`stream: "typed_memory_relations"`, `journal_id`, unsigned 64-bit `sequence`
and `prefix_digest`. Preserve the complete value exactly. Memory v1/v2 cursors
are not interchangeable with relation cursors. Use lossless integer handling in
clients that cannot represent every unsigned 64-bit number exactly.

The change contains `schema_version`, `kind`, `relation_id`, `relation_revision`,
exact `source` and `target` memory references, `relation_kind`, authenticated
`author`, `committed_at_millis`, `relation_digest` and `receipt_digest`. Kinds are
`asserted`, `retired`, `restored` and `reviewed`. These are metadata notifications;
no memory text, vectors, extraction output or provenance evidence text is copied
to this journal. Review events do not claim that the relation remains eligible.
Refetch current authorized state rather than reconstructing current truth from an
old event. Retired or rejected relations can still appear in historical metadata.

The relation value, revision history, idempotency receipt, both adjacency indexes,
completeness headers and change entry commit in one synchronous WAL batch. An
identical command retry returns its original receipt without another event.
Rejected commands and conflicts do not advance the journal. Checkpoint writes and
journal activation create no relation events. Ordinary memory mutations continue
to use the memory journal only.

## Catch up without following a moving fence

1. Request a page using the last durable cursor as `after` and null `through`.
2. Retain the returned `high_watermark`. Reuse it as `through` on subsequent
   pages and advance `after` only after the corresponding local effects are durable.
3. Deduplicate deliveries by the full stream identity and position. Commit the
   local effects and exact cursor witness atomically whenever the destination
   supports it. Use destination idempotency and explicit reconciliation otherwise.
4. When `complete` is true, retain `next_cursor`. Clear `through` for a later
   polling cycle so it can observe new commits.

A finite catch-up fence is not a retained server pagination session or an
indefinite freshness lease. Repeating a page can redeliver events. This does not
promise exactly-once external effects, a distributed worker lease or automatic
retries. Set bounded retry/backoff policies and allow cancellation between calls.

## Establish an initial baseline

The first newly committed relation mutation activates the journal atomically.
An authorized writer can also activate it explicitly using:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "private"}
}
```

Activation returns the original `baseline`, even after later events exist.
Concurrent activation is idempotent. Read the feed to obtain the **current**
high watermark before reconciling a destination; an activation retry is not a
shortcut to the current tip.

For initial graph-cache synchronization, capture current memory and relation
watermarks, reconcile the desired authorized roots and graphs, then process
changes after those watermarks. Refetch affected roots or invalidate the scoped
cache when the dependency set is unknown. Record both stream witnesses after
durable effects; neither substitutes for the other. This is eventual
reconciliation across requests, not an atomic snapshot spanning both feeds.

Existing relations, history and command receipts survive journal introduction.
The upgrade does not invent events for pre-journal assertions. Replaying an old
command also creates no synthetic history. An external worker needing an initial
corpus must enumerate authorized memories and selected typed neighborhoods;
this feed is not an all-relations inventory API. Do not run older experimental
relation writers against a volume after adopting this journal: those writers do
not maintain its delivery history.

## Commit owned progress

A checkpoint belongs to `(authorized namespace, authenticated subject,
consumer_id)`. Use a distinct consumer for each destination binding. Credential
rotation within the same subject retains ownership; a different subject cannot
read or change that progress. Do not reuse a consumer ID for an unrelated sink.

After initial reconciliation has durably retained an actual cursor, initialize
with `expected_revision: 0`, a null or omitted expected digest and `advance`:

```json
{
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "private"},
  "command": {
    "contract_version": 1,
    "idempotency_key": "relation-cache-initialize-1",
    "consumer_id": "context-cache",
    "expected_revision": 0,
    "expected_checkpoint_digest": null,
    "operation": {
      "type": "advance",
      "cursor": {
        "version": 1,
        "stream": "typed_memory_relations",
        "journal_id": "018f0000-0000-4000-8000-000000000010",
        "sequence": 1,
        "prefix_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      }
    }
  }
}
```

The illustrated cursor is a placeholder, not an accepted server cursor. Subsequent
advances require both the current checkpoint revision and its complete
`checkpoint_digest`. The server verifies the old and new cursor against current
history, rejects backward progress, and atomically commits current progress,
immutable checkpoint history and the idempotency receipt. A lost comparison
returns 409; do not replace its expectations and blindly retry the same effects.

Consumer IDs allow 1–128 UTF-8 bytes, idempotency keys 1–256 bytes; neither may be
blank or contain control characters. Idempotency is scoped to the subject and
namespace, across checkpoint operations. A changed command under an existing key
returns 409. Initializing an already existing checkpoint also conflicts.

A receipt contains `previous`, `checkpoint`, `reconciliation_evidence` and
`receipt_digest`, plus contract version and idempotency key. `previous` is null
only for initialization. Every successful command increments the checkpoint
revision, even when its cursor remains unchanged. Reconciliation evidence is null
for an ordinary advance. Replaying a command can return an old receipt after
progress advances or history diverges. **A receipt proves the historical command;
it must not substitute for reading current progress.**

Read current progress with:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "private"},
  "consumer_id": "context-cache"
}
```

To read an immutable receipt, send that body with a positive `revision` to
`/checkpoints/revision`. Historical receipts can remain readable when their
cursors no longer belong to the current journal. They remain audit evidence,
not permission to skip or undo external effects.

## Diagnose and reconcile a divergent history

Send the current-read body to `/consumers/diagnose`, optionally adding `witness`
with the exact relation cursor persisted by the destination. Diagnostics use one
storage snapshot and do not alter progress.

| Diagnostic field | Meaning |
| --- | --- |
| `active`, `baseline`, `high_watermark` | Current scoped relation history |
| `checkpoint` | Retained subject-owned checkpoint, including incompatible progress |
| `checkpoint_status` | `missing`, `compatible` or `history_incompatible` |
| `pending_positions` | Distance from compatible checkpoint to current tip; otherwise null |
| `witness_status` | `not_provided`, `compatible` or `history_incompatible` |
| `checkpoint_relative_to_witness` | `before`, `equal` or `after` when both cursors match current history; otherwise null |

A checkpoint before the durable witness can result from a lost acknowledgement.
A checkpoint after the witness means the destination cannot demonstrate effects
already acknowledged by the server. Incompatible history requires reconciliation;
never reset to zero or adopt the current tip silently. Pending positions are not
a count of valid relations or proof that every unexamined event is intact.

After reconciling external effects, explicitly submit `operation.type: reconcile`
with an actual current cursor and a nonempty `evidence_ref` of at most 2048 UTF-8
bytes. Supply the exact existing checkpoint revision and digest. This can replace
an incompatible cursor or intentionally rewind compatible progress. It cannot
initialize a missing checkpoint, bypass a stale comparison or accept a cursor
from a foreign history. The receipt permanently retains the prior checkpoint,
new checkpoint, authenticated author and declared evidence reference. The server
does not independently certify the external reconciliation described by that
reference. No automatic repair or rewind occurs on a failed ordinary read.

## Integrity, retention and operational limits

Each event is linked to the previous prefix with a digest and fresh nonce,
bound to the exact namespace and immutable relation revision/receipt. A divergent
restore is rejected even after it repeats the same command or passes the old
sequence number. A current tip, supplied cursor boundaries and delivered entries
are checked; consecutive delivered prefixes must connect. Missing expected data
or encountered inconsistent metadata fails the whole request without a partial
page. Checkpoints retain immutable revisions; an orphaned history cannot silently
become a missing checkpoint that a client can overwrite.

A page examines at most 256 delivered events, with additional bounded tip and
cursor-boundary checks. Stored event values are limited to 4 KiB, and the bound
relation history values to 16 KiB. Checkpoint receipt values are limited to 16 KiB.
These logical bounds do not describe total RSS or encoded HTTP size. A read may
fetch a crossing value before validating its length. Diagnostics and an empty
catch-up page do not constitute a full-history corruption audit. Coordinated
rewriting of all digests is outside this integrity model.

No automatic pruning or cursor expiry is provided. Back up the journal,
checkpoints, immutable histories and canonical relation data together. A local
witness restored with the database cannot reveal effects that both restored
copies forgot; preserve the destination's actual durability boundary.

| HTTP status | Action |
| --- | --- |
| 400 | Correct malformed versions, fields, bounds, identities or evidence |
| 401 / 403 | Resolve current credentials, capabilities and scope; retain progress |
| 404 `checkpoint_not_found` | The owned checkpoint or requested revision does not exist; do not infer that the external sink is empty |
| 409 `journal_history_conflict` | Stop incremental application and reconcile history |
| 409 `revision_conflict` | Another write changed progress; reread before deciding what to do |
| 409 `idempotency_conflict` | Preserve the old key for its exact command; investigate changed content |
| 409 `checkpoint_regression` | An ordinary advance would move backward; reconciliation must be explicit |
| 413 | Reduce the request below 65,536 bytes |
| 500 | Investigate encountered storage inconsistency; never accept a partial page |
| 503 `retrieval_busy` | Retry later with bounded backoff and unchanged durable progress |

The existing Python `VerifiedMemoryConsumer` targets the memory v2 stream and
must not be pointed at these routes. This release exposes the relation HTTP and
Rust contracts; a packaged relation consumer adapter is separate work. Expiration,
revocation and endpoint changes still require current-state checks. These
contracts establish consumption integrity, not evidence that a graph improves
retrieval, reasoning or autonomous learning.
