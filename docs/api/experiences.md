# Experience receipts API

Availability: **0.7.0, unreleased**. Check `/health` on the server you use; a
running 0.6.0 server does not expose these routes.

Use experience receipts to preserve what an agent attempted, which execution
context it declared, who reported the result and what consumption remains
unknown. Registrations and observations survive restart. An observation is an
authenticated assertion: storing `succeeded` does not verify an external effect,
qualify a procedure, dispatch a tool or update model weights.

The API uses the same durable learning store as [procedural learning](procedural-learning.md).
Procedure qualification retains its existing policy and evaluator authority.
Models, embeddings, execution and evidence verification remain external services.

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
source-revocation propagation, evidence deduplication, independent execution
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
