# Experience receipts API

Availability: **0.7.0**. Check `/health` on the server you use; a
running 0.6.0 server does not expose these routes.

Use experience receipts to preserve what an agent attempted, which execution
context it declared, who reported the result and what consumption remains
unknown. Registrations and observations survive restart. An observation is an
authenticated assertion: storing `succeeded` does not verify an external effect,
qualify a procedure, dispatch a tool or update model weights.

The API uses the same durable learning store as [procedural learning](procedural-learning.md).
Procedure qualification retains its existing policy and evaluator authority.
Models, embeddings, execution and verification of external effects remain external services.

## Routes and authority

Every request uses `Authorization: Bearer <credential>`, `contract_version: 1`
and an exact resource `scope`. Tenant and actor are derived from the live
credential. Request fields such as `actor`, `tenant` or `credential_id` are not
accepted. Responses use `Cache-Control: no-store`. JSON bodies are limited to
65,536 bytes, including all field names and encoded strings.

| Method and route | Required capability | Response field |
| --- | --- | --- |
| `POST /api/v1/experiences` | `experience_write` | `receipt`: immutable registration |
| `POST /api/v1/experiences/read` | `experience_read` | `experience`: current attempt state |
| `POST /api/v1/experiences/events` | `experience_report` and the registered reporter subject | `event`: immutable observation and resulting state |
| `POST /api/v1/experiences/events/read` | `experience_read` | `event`: the requested historical observation |
| `POST /api/v1/experiences/history` | `experience_read` | `page`: revision-fenced event history |
| `POST /api/v1/experiences/artifacts` | `experience_report`, `tool_read` and the registered reporter subject | `binding`: immutable stored-artifact link |
| `POST /api/v1/experiences/artifacts/read` | `experience_read` and `tool_read` | `binding`: the requested artifact link |
| `POST /api/v1/experiences/lineage` | `experience_read` | `lineage`: bounded pinned ancestry |
| `POST /api/v1/experiences/export` | `experience_read` | `export`: exact selected observations and accounting summary |

Capabilities are independent. Memory, tool and procedure permissions do not grant
experience access. A writer chooses `reporter_subject_id` when registering an
attempt; this binding grants no credential or capability. Reporting requires a
current credential for that subject with `experience_report` and the exact scope.
Grant `experience_read` separately when the worker needs to inspect state.

Shared scopes permit distinct writer and reporter subjects within the same tenant
and exact grant. A private attempt must name its owning subject as the reporter.
Private namespaces of two subjects remain distinct even if every visible resource
identifier is identical. Project, mission and agent are exact values; `null`
mission is not a wildcard. See [authentication](../security/scoped-credentials.md).

## Register an attempt

First have a policy administrator register the immutable execution context through
[`/api/v1/learning/contexts`](procedural-learning.md#register-the-administrative-contracts).
That context identifies the task, baseline, model, tools, environment, evaluation
contract, dataset, harness and permissions. Experience registration binds its
server-computed content digest. Tool identities in that context remain declarations;
this API does not resolve them into executable artifact bytes.

Then post the following to `/api/v1/experiences`:

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "mission_id": null,
    "agent_id": "agent",
    "visibility": "shared"
  },
  "request": {
    "id": "attempt-v1",
    "context_id": "context-v1",
    "reporter_subject_id": "observer",
    "accounting_unit": "test-credit-v1",
    "input": {
      "reference": "fixture:input",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    "parent": null
  }
}
```

The repeated digest is a synthetic placeholder. Supply the actual SHA-256 of the
referenced input in an integration. The server preserves the reference and digest
without fetching their content. `accounting_unit` names your versioned consumption
unit; the database does not infer currency, price or provider billing.

The response contains `contract_version: 1` and `receipt`, with the original
`request`, authenticated `actor`, `tenant`, `namespace`, `context_digest`, optional
`parent_event_digest`, server `recorded_at_millis`, `schema_version` and
`receipt_digest`. Save the returned `context_digest` for observations.

Registration starts at revision **1**, with `outcome: null` and no event. It records
intent only. An identical request ID and content from the same subject returns the
original receipt, including its original credential and timestamp. Changed
content or a different creating subject returns 409. A rotated credential for the
same subject may retry after current authorization succeeds.

## Record an observation

Post to `/api/v1/experiences/events` as the bound reporter. Replace the example
context digest with the exact value returned during registration:

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "mission_id": null,
    "agent_id": "agent",
    "visibility": "shared"
  },
  "attempt_id": "attempt-v1",
  "command": {
    "event_id": "observation-v1",
    "expected_revision": 1,
    "context_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    "outcome": "unknown",
    "evidence": {
      "reference": "fixture:trace",
      "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    },
    "cost_units": null,
    "latency_ms": null
  }
}
```

The event records the command, authenticated actor, resulting `record`, server
timestamp, schema version and `event_digest`. It advances the attempt to revision
2 in the same synchronous write batch. The evaluation/verifier contract identity
comes from the immutable registered context; the credential identifies the
reporter. Neither establishes that the referenced verifier actually ran.

| Current outcome | Accepted new outcome | Meaning |
| --- | --- | --- |
| `null` | `unknown`, `succeeded`, `failed`, `cancelled` | First observation |
| `unknown` | Any of the four outcomes | Further uncertainty or a reported result |
| `succeeded`, `failed`, `cancelled` | The same outcome only | Additional evidence or late consumption, preserving the terminal outcome |

`cancelled` is a reporter assertion, not a cancellation request or proof that a
remote process stopped. An execution with unknown completion stays `unknown`.
Record corrections that require a different terminal outcome as a new, explicitly
linked attempt; this release does not replace or automatically supersede history.

Event IDs are unique within an attempt. Identical command retries by the same
subject return the original event even after later observations. Changed commands
under the same event ID return 409. For a new event, `expected_revision` must match
current state. Concurrent different events using one revision have one winner;
losers must read current state before deciding whether to submit a new observation.
Never change an event ID merely to make an uncertain retry look new.

## Preserve unknown resource consumption

`cost_units` and `latency_ms` are cumulative observations for the whole attempt,
not increments. Missing or `null` means unknown; **0 means a reported zero**.
Known cumulative values cannot decrease relative to an earlier known value.
Different attempts may use different accounting units; do not sum them without
an explicit conversion policy.

The current record distinguishes two views:

| Fields | Meaning |
| --- | --- |
| `reported_cost_units`, `reported_latency_ms` | Values in the latest observation, possibly unknown |
| `observed_cost_units`, `observed_latency_ms` | Greatest known cumulative observations, retained when a later report is unknown |

For example, a report of cost 9 followed by `null` produces reported cost `null`
and observed cost 9. The latter is a retained lower bound, not evidence that total
cost is exactly 9. A later same-outcome report may supply 12. It cannot supply 8.
An outcome may be `succeeded` while cost remains unknown; admission of that receipt
is not budget qualification. Existing procedural evaluation still requires its
own valid evidence and accounting.

## Read current state and historical evidence

Post to `/api/v1/experiences/read`:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "attempt_id": "attempt-v1"
}
```

For `/api/v1/experiences/events/read`, add `"event_id": "observation-v1"`.
That operation returns the historical event, not the latest state. There is no
cursor, listing, automatic graph traversal or semantic search over experiences
in this version. Server wall-clock timestamps are audit metadata; revision and
event identity define the per-attempt order.

To branch from an observed experience, create a new attempt with:

```json
{"parent": {"attempt_id":"attempt-v1","event_id":"observation-v1"}}
```

The parent event must already exist in the same authorized namespace. The server
binds its exact `event_digest`; later parent observations do not alter the child.
An attempt cannot name itself as parent. References to existing events support an
acyclic creation history, but do not implement a replay simulator. Inputs from
another execution context remain distinguishable through each attempt's context.

## Limits, errors and durability

| Condition | HTTP status and code |
| --- | --- |
| Invalid JSON, extra fields, bad digest, decreasing consumption or invalid private reporter | `400 invalid_request` |
| Unsupported contract version | `400 unsupported_contract_version` |
| Missing, expired, revoked or rotated-old credential | `401 unauthorized` |
| Missing capability/grant or wrong reporting subject | `403 forbidden` |
| Context, attempt or parent/event absent from the authorized namespace | `404 record_not_found` |
| Stale expected revision | `409 revision_conflict` |
| Changed request/event, mismatched context digest or terminal outcome | `409 idempotency_conflict` |
| Body exceeds 65,536 bytes | `413 invalid_request` |
| Stored identity, digest or receipt/state inconsistency | `500 storage_inconsistency` |

Record IDs, context IDs, event IDs and accounting units support 1–512 UTF-8 bytes;
reporter subjects support 1–256 without control characters. Evidence references support 1–2048 bytes. Values
must be nonblank. Digests use exactly 64 lowercase hexadecimal characters, without
a `sha256:` prefix. Consumption values are unsigned 64-bit integers. Revisions
start at 1; a submitted revision must allow its unsigned 64-bit successor.

Acknowledged writes synchronize the RocksDB WAL. Each observation and resulting
state commit atomically. Process-kill tests exercise recovery of registrations,
events and idempotency; they do not establish hardware power-loss tolerance.
Authorization is checked on each request, including retries. An operation already
authorized may finish while revocation races with it, as in the existing platform
authorization contract.

Server receipt/event digests identify serialized records; use the returned values
as opaque identities rather than reimplementing their serialization in clients.
They detect inconsistent records, not malicious filesystem operators or false
external evidence. This release has no experience deletion, retention policy,
automatic source-revocation inference, evidence deduplication, independent execution
verification or automatic strategy extraction. See the
[experience-memory design](../research/experience-memory-design.md) for the
separate acceptance criteria for those extensions.

## Read a bounded observation history

`POST /api/v1/experiences/history` requires `experience_read` for the exact scope.
Use this endpoint to audit an attempt without remembering every event ID:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "attempt_id": "attempt-v1",
  "query": {"limit": 32, "cursor": null}
}
```

The response's `page` contains `receipt_digest`, `through_revision`, `events`,
`scanned_events`, `next_cursor` and `complete`. Pass `next_cursor` unchanged in the
next request. Stop only when `complete` is true. Every continuation rechecks the
credential and scope; a cursor grants no access and is bound to the attempt receipt.

The first page freezes the attempt's current revision. Later reports are omitted
from that traversal. This is a **logical revision snapshot** of immutable events,
not a retained database snapshot. Event IDs are ordered by UTF-8 byte length, then
by byte value, matching storage keys; this is not chronological order. Sort a
completed export by each event's `record.revision` when chronology is required.

`limit` is a candidate budget from 1 through 64. Each request examines at most
that many event candidates; authentication, receipt and fence lookups are
additional fixed reads. A page can be empty and still have a cursor when newly
appended events fall beyond the fence. A full final page can require an extra
empty continuation. New reports can increase scan work, but never enter the frozen
result set or duplicate previously returned events when the cursor is preserved.
The service retains no pagination session, so continuation survives restart.

An absent attempt returns 404. A cursor for a different receipt returns 409;
invalid bounds or missing continuation/fence events return 400. This endpoint
also works for attempts written before history pagination was introduced.

## Bind a stored tool artifact to an observation

When an external worker has used or produced a tool, it can attach a verified
**stored artifact identity** to an exact observation. Register the immutable
[tool artifact](learned-tools.md) first. Then call
`POST /api/v1/experiences/artifacts` with `experience_report` and `tool_read` in
the same scope, using the attempt's bound reporter subject:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "attempt_id": "attempt-v1",
  "binding": {
    "id": "binding-v1",
    "event_id": "observation-v1",
    "artifact_id": "tool-v1",
    "artifact_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "role": "candidate"
  }
}
```

Replace the example digest with `artifact_digest` returned by artifact registration.
The database verifies the stored source, dependency lock and complete artifact
proposal digests, and rejects a mismatched submitted digest with 409. The artifact
and observation must already exist in the exact authorized namespace; absence
returns 404. The roles `baseline`, `candidate` and `output` describe the reporter's
assertion. They do not prove execution, fitness, independent evaluation or use by
a particular model.

The returned `binding` includes the immutable request, attempt ID, original
`receipt_digest`, exact `event_digest`, verified `source_digest` and
`dependency_digest`, authenticated actor, server time and opaque `binding_digest`.
It contains no source code. Binding does not revise the attempt, change its
outcome or qualify a procedure. Late bindings are allowed and do not establish
that the artifact existed when an external execution occurred.

A binding ID is immutable within an attempt. An identical request from the same
reporter subject returns the original binding, including after credential
rotation or restart. Conflicting reuse returns 409. All writes synchronize the
WAL. Read it with `POST /api/v1/experiences/artifacts/read` using the usual
`contract_version`, `scope`, `attempt_id` and `binding_id`. Reading requires both
`experience_read` and `tool_read`; every read revalidates the artifact and event.
Multiple bindings to one artifact do not constitute independent evidence.

## Trace the recorded lineage

`POST /api/v1/experiences/lineage` reads the exact parent observations pinned by
an attempt. It requires `experience_read` for the exact scope:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "attempt_id": "attempt-v1",
  "max_depth": 16
}
```

The response's `lineage` contains the immutable `origin` receipt and `ancestors`
in immediate-parent-to-root order. Each ancestor is the complete historical event
whose digest was pinned by its child. A parent that later succeeds, fails or
receives a consumption update does not change this historical path. A root attempt
returns an empty ancestry and `complete: true`.

`max_depth` is required and allows 1–64 ancestor events. When the limit is reached
before the root, `complete` is false and `next_parent` plus `next_parent_digest`
identify the next unread link. To continue, request the lineage of the **last
returned ancestor's attempt ID**. Its immutable receipt starts at the next link;
concatenate the ancestor arrays without repeating the previous page's origin.
Each request rechecks authorization and scope. The response is not a live-state
snapshot, a listing of descendants or an enumeration of possible branches.

The reader fails closed when a visited parent is missing, its digest disagrees
with the child, or a visited cycle is detected. An absent requested origin returns
404; malformed depth returns 400; inconsistent stored lineage returns 500.
A truncated path reports its limit explicitly and is never labeled complete.

This records declared provenance. It does not prove a causal dependency, replay
an execution, invent unobserved transitions or certify the truth of a report.

## Export an exact observation cohort

`POST /api/v1/experiences/export` requires `experience_read` and accepts an explicit
set of 1–64 observations, with one event per distinct attempt. Supply each exact
`event_digest`, the common `context_digest` and the common `accounting_unit`:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "selection": {
    "context_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "accounting_unit": "test-credit-v1",
    "events": [{
      "attempt_id": "attempt-v1",
      "event_id": "observation-v1",
      "event_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }]
  }
}
```

Use digests returned by the service, replacing the placeholders above. The
response's `export` contains full immutable events sorted by attempt ID in UTF-8
byte order, `method_version: "qilbee.experience-export.v1"`,
`coverage: "explicit_event_set"`, the common context and accounting unit, a
`summary`, and `export_digest`. Request order does not affect the output. Later
observations cannot alter an exported historical event. The server generates the
same output for the same event set after restart; it does not create an export
job, retain a snapshot session or write a new ledger record.

The summary counts `attempts`, `succeeded`, `failed`, `cancelled` and `unknown`.
For each of `cost_units` and `latency_ms`, it reports:

| Field | Meaning |
| --- | --- |
| `known_reports` | Selected events with a non-null reported value, including zero |
| `unknown_reports` | Selected events with a null reported value |
| `reported_total` | Sum of reported values, or null if any report is unknown |
| `observed_lower_bound` | Sum of retained known cumulative lower bounds; never a claim of complete consumption |

Totals are exact unsigned decimal **strings**, allowing sums beyond unsigned
64-bit range without rounding. Individual event values keep their existing
unsigned 64-bit representation; use a lossless JSON parser when needed. A known
reported value is still the reporter's cumulative observation, not a certified
final bill. An unknown outcome remains a separate count.

The response is all-or-error: no partial cohort is returned. Missing or
out-of-scope events return 404; mismatched event hashes, context or accounting
unit return 409; duplicate attempts or an invalid count return 400. The standard
65,536-byte request-body limit also applies. No event is replaced with its latest
state, excluded silently, or inferred from a parent. Missing observations and
attempts outside the submitted set are not included in the denominator.

The export covers exactly the submitted set, not the entire namespace, a random
sample or independently verified trials. Artifact bindings and ancestors are
available through their own endpoints; the export does not implicitly include
them. See [evaluate experience evidence](../research/experience-evaluation.md)
for a reproducible comparison workflow and the limits of these summaries.


## Withdraw an observation from future reuse

An authorized operator can withdraw an exact observation without erasing its
history. Use the same API on the managed platform or your self-hosted instance.
The company decides whether evidence should be withdrawn; QilbeeDB records that
decision and enforces it through registered dependencies. It does not judge the
truth of the observation or cancel an agent action already in progress.

Send `POST /api/v1/experiences/withdrawals` with `contract_version: 1`, the exact
`scope`, an `idempotency_key`, an `observation` containing `attempt_id`, `event_id`
and the server-returned `event_digest`, and a nonblank `reason`. This requires
both `experience_evidence_admin` and `experience_read` for that scope. The new
administrative capability must be granted explicitly. It does not permit access
to another private subject's namespace.

For both withdrawal endpoints, bodies exceeding 16,384 bytes return
`413 invalid_request`. Within that limit, an unsupported contract version returns
`400 unsupported_contract_version`; invalid JSON, fields or lengths return
`400 invalid_request`. Clients must not silently downgrade the contract version.

The request body is limited to 16,384 bytes. Idempotency keys allow 128 UTF-8
bytes and reasons allow 2,048; neither accepts control characters. Retain the
returned receipt. After a lost response, retry the exact command with the same
key or call `POST /api/v1/experiences/withdrawals/inspect` with the scope and exact
observation. Never assume a timeout means the write failed. Reusing a key with a
different command is a conflict. A new key for an already withdrawn observation
returns `409 experience_already_withdrawn`; inspect the existing receipt.

Withdrawal is irreversible in this contract. Historical events, exports,
proposal receipts and evaluation results retain their original contents and
digests. An old receipt or an `Active` qualification is not proof that its
evidence remains eligible for reuse. Parent-attempt links alone do not propagate
withdrawal, and the database cannot identify copied evidence without registered
lineage. Withdrawal is not physical deletion or a retention policy.

Before reusing an ordinary strategy, call
`POST /api/v1/learning/strategies/inspect` with `contract_version: 1`, `scope` and
`strategy_id`, or perform current selection. Inspection requires `memory_read`
and returns current eligibility without protected observation details, withdrawal
reasons or actor identities. Repeat this check after reconnecting. The memory
change feed does not replace experience eligibility checks.

The server applies the following fixed logical-read budgets. These are request
ceilings, not latency guarantees or measurements of physical disk traffic.

| Operation | Logical records | Logical bytes |
| --- | ---: | ---: |
| Withdraw or inspect an exact observation | 32 | 2 MiB |
| Admit or replay an ordinary or combined experience proposal | 4,096 | 32 MiB |
| Inspect current ordinary strategy eligibility | 256 | 8 MiB |
| Select through the legacy learning endpoint | 4,096 | 16 MiB |
| Inspect current combined knowledge eligibility | 512 | 16 MiB |

Admission budgets cover the whole request, including registered dependencies;
combined admission also accounts for memory-origin validation. Legacy selection
examines at most 1,000 procedure entries. Existing v4 knowledge-selection budgets
remain in effect. JSON serialization can increase the stored size of escaped
characters, so a limit on input string bytes does not equal a logical-read budget.
Retained records exceeding a current verification budget remain stored; an
incomplete check does not make them eligible for reuse.

Current checks have bounded work. `422 experience_reuse_limit_exceeded` means
verification is incomplete, not that no useful knowledge exists. Preserve the
cached item as unavailable for reuse and investigate the limit; do not fall back
to an old eligible response. `503 retrieval_busy` permits a later read retry
with backoff. A selection observed before withdrawal commits may finish; a later
observation excludes registered withdrawn evidence. Already delivered model
context cannot be retracted by this API.


### Self-hosted upgrade and retained evidence

Upgrades build a derived locator for historical strategies so both native and
HTTP selection can resolve their experience dependencies. Plan a maintenance
window and keep exclusive ownership of the data directory until migration
completes. Do not run an older binary or another writer between migration passes.

Each pass is bounded by 100,000 receipts, 1 GiB of logical read bytes and
4,000,000 logical reads. If a pass reaches its limit, startup stops with an
explicit error and retains an acknowledged checkpoint. Restart the same version
to continue after the last completed receipt. A single receipt still has its own
verification budget; restarting cannot bypass an oversized or corrupt receipt.
Completion publishes the locator marker atomically and clears the checkpoint.

The checkpoint verifies its last receipt and locator; it is not protection
against an external writer changing earlier records. If another version wrote
to the store during the upgrade, do not assume the checkpoint is safe to resume.
Use a verified backup and reconcile the interrupted migration. The offline store
verifier rejects incomplete locator migrations rather than reporting a complete
store. Managed-platform users do not perform this local maintenance procedure.
