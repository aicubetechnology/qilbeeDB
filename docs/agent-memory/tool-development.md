# Durable tool development

The learning store owns development requests, repair provenance and worker
reports. It does not generate source, invoke models or run tests. An independently
deployed development worker performs that work and submits authenticated evidence.
The platform never interprets a stored artifact as executable code.

## Request binding

`create_tool_development` binds one immutable request ID to an objective,
administrator-registered executor ID and optional repair parent/evidence. The
original receipt preserves request and executor digests, tenant, namespace,
requesting subject, credential and timestamp. An identical retry by the same
subject returns that receipt, including after credential rotation and later
state changes. Changed content or a different requesting subject conflicts.
Repairs require an existing artifact in the same namespace plus an evidence
reference. References to external test systems are declarations, not fetched or
independently verified proof.

`tool_development` reads the current record, beginning with revision 1 and state
`requested`. Creation records intent only; it does not enqueue or dispatch work.
A configured worker needs the request ID, the same shared scope and the required
platform credentials. Private scopes remain isolated by subject.

## Events, authority and persistence

`apply_tool_development` accepts an immutable event ID, expected current revision
and either a worker report or cancellation request. Reports require the exact
registered executor subject and profile digest. Cancellation requires the
original requesting subject. Network handlers additionally enforce distinct
capabilities and exact scope grants. Library callers are trusted to establish
those grants. A repeated event with the same subject and complete command returns
its original event snapshot; changed reuse conflicts. A new event with a stale
revision conflicts. The new event, current state and successful artifact are
written in one batch with a synchronized WAL.

`tool_development_event` reads an original event snapshot. Readers verify
request/profile bindings, the current state's originating event and successful
artifact existence/digest. A command's raw reported outcome and the platform's
resulting state are both retained, including when the two differ.

## Outcomes and cancellation

| Worker report or command | Result |
| --- | --- |
| Success with matching artifact, known accounting and values within both budgets | `succeeded`, exact artifact stored atomically |
| Success with unknown cost or latency | `pending_or_unknown`, no successful artifact registration |
| Success exceeding either known resource limit | `failed`, reason `resource_limit_exceeded` |
| Reported failure | `failed`; failure evidence and optional detail remain in the event |
| Pending/unknown report | `pending_or_unknown`; no implied completion or redispatch |
| Request cancellation | `cancellation_requested`; no claim that the worker stopped |
| Worker confirms requested cancellation | `cancelled`; unknown resource values remain null |

A pending/unknown report cannot erase an outstanding cancellation request. A
success with unknown consumption also preserves cancellation intent. A complete,
valid success may win a cancellation race before cancellation is confirmed.
After any terminal state (`succeeded`, `failed`, `cancelled`), new commands are
rejected. Identical old event retries still return their original snapshots.
A new attempt or repair uses a new request ID. There is no automatic retry,
lease expiry, failover or redispatch that could duplicate external effects.

Reported cost and latency are cumulative totals for the development request;
known totals cannot decrease, even across a later unknown report. The separate
`observed_cost_units` and `observed_latency_ms` fields retain the last known lower
bounds; current `cost_units` and `latency_ms` can remain null. Null means unknown,
never zero. Terminal failure
or cancellation may still have unknown consumption. Budgets gate acceptance of a
successful report, and do not enforce limits inside an external operating system.

A successful artifact must match the bound runtime image and request's exact
parent/evidence pair, and include `development:<request ID>` in `source_refs`.
Missing/mismatched artifact fields are request errors or conflicts and leave
state unchanged. Success means the worker reported development success within
this contract. It does **not** qualify efficacy, approve publication, or authorize
invocation. Statistical tool evaluation and release gates remain separate work.

## Bounds

Request and event IDs and executor IDs support 1–512 UTF-8 bytes. Objectives and
worker details support 1–8192 bytes. Evidence references and cancellation reasons
support 1–2048 bytes. Reports and artifact source must also fit the HTTP transport
limit. No credentials or private dataset contents should be embedded in source,
details or references. Events are durable and individually addressable; this
interface does not scan or return an unbounded event history.
