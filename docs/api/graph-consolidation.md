# External graph consolidation

Status: **unreleased implementation**. The production 0.13.0 API does not expose
these routes. Consolidation runs model inference in your application or worker;
QilbeeDB coordinates durable jobs, exact source revisions and atomic publication.
The database does not generate embeddings, call a model provider or accept its
credentials. This contract does not establish a retrieval or agent-task quality gain.

Use consolidation to turn a bounded set of memories into
[typed assertions](typed-memory-relations.md). Each accepted output depends on the
entire declared context, including memories that are not its two endpoints.
An extractor must declare every memory it uses. A database cannot discover context
that the caller omitted, or independently certify a generated assertion as true.

## Authorization and ownership

All routes use POST, `Authorization: Bearer <scoped-api-key>`, JSON and
`contract_version: 1`. Company, project, agent, mission, visibility and private
subject are authorized before inspecting work. The authenticated subject owns
the job within that partition, including in a shared memory scope. Another subject
with access to the shared memories does not own that subject's jobs.

| Route | Capabilities | Purpose |
| --- | --- | --- |
| `/api/v1/memory/consolidation/commands` | `memory_read`, `memory_write` | Create, claim, renew, publish, fail, recover, cancel or reconcile usage |
| `/api/v1/memory/consolidation/inspect` | `memory_read` | Retained current job, source diagnostic and current lease status |
| `/api/v1/memory/consolidation/revision` | `memory_read` | Exact immutable job revision and its receipt |
| `/api/v1/memory/consolidation/query` | `memory_read` | Bounded discovery of the caller's current jobs |
| `/api/v1/memory/consolidation/context` | `memory_read`, `memory_write` | Exact source payloads for the current credential-bound lease |

`worker_id` is a caller label, not an authenticated identity. The server binds each
lease to the actual credential ID, an unpredictable fence and the current storage
incarnation. A second credential for the same subject can inspect the job but
cannot use the first credential's execution fence. Revoked credentials fail normal
authorization before a receipt can be replayed or a context read can succeed.

A consolidation job grants no additional source, review, tool-execution or company
administration authority. Its `policy_ref` identifies the caller's policy; it is
not an independently verified learning-policy approval. Published assertions use
the existing relation eligibility and review rules.

## Create a job with a frozen manifest

Send this envelope to `/commands`, replacing the fixture IDs and identities:

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "mission_id": null,
    "agent_id": "agent",
    "visibility": "private"
  },
  "idempotency_key": "consolidation-session-42",
  "operation": {
    "type": "create",
    "spec": {
      "sources": [
        {"record_id": "018f0000-0000-4000-8000-000000000001", "revision": 2},
        {"record_id": "018f0000-0000-4000-8000-000000000002", "revision": 1},
        {"record_id": "018f0000-0000-4000-8000-000000000003", "revision": 4}
      ],
      "objective": "Extract entity and evidence links supported by these memories.",
      "policy_ref": "policy://entity-consolidation/v3",
      "extractor": {
        "origin": "model_inference",
        "method": "entity-and-evidence-extraction",
        "method_revision": "prompt-v3",
        "evidence_ref": "trace://session-42/manifest",
        "model": {
          "provider": "example-provider",
          "model": "example-model",
          "revision": "example-pinned-revision"
        }
      },
      "max_relations": 16,
      "max_attempts": 3,
      "lease_millis": 30000,
      "max_attempt_millis": 300000
    }
  }
}
```

The manifest contains 2–16 distinct IDs with exact positive revisions. All must be
current and transitively eligible in the same scope. Creation rejects an unavailable
source or a context that cannot fit the bounded context response. The objective,
policy reference, extractor and limits are immutable. Create a new job to change
its context or extraction policy; do not reuse its idempotency key for new intent.

The result is `contract_version` and `receipt`. The receipt identifies `job_id`,
`revision`, `action`, original author and commit time, plus opaque job, command and
receipt digests. Store it durably. These integrity values are not signatures.

## Claim and read the inputs

Keep the command envelope and change `operation` to:

```json
{
  "type": "claim",
  "job_id": "018f0000-0000-4000-8000-000000000010",
  "expected_revision": 1,
  "worker_id": "extractor-worker-7"
}
```

Use a distinct idempotency key for each intended operation. Competing claims with
the same expected revision have at most one winner. A successful claim creates an
attempt, a new fence and a server-clock expiry. It records unknown consumption
before any external request, so a crash cannot silently become a zero-cost run.

Inspect the current job with `contract_version`, `scope` and `job_id`. The response
contains `inspection.job`, `lease_active`, `recoverable`, `source_failure`, the
evaluation time and dependency work. A replayed claim receipt is historical: it
does not prove that its lease is still active. Verify the current attempt before
invoking an external provider.

Request `/context` with the same envelope, `job_id`, the current job revision as
`expected_revision`, and the attempt's `fence`. It returns exact eligible source
records in manifest order, evaluated in one snapshot and clock. Another credential,
a stale revision, an expired fence or a previous storage incarnation is rejected.
A context response is not a freshness lease for the memories: publication checks
all sources again after inference finishes.

Long-running workers can issue `renew` with `job_id`, `expected_revision` and
`fence`. Renewal advances the job revision and extends the lease up to the fixed
`max_attempt_millis` deadline measured from the original claim. It keeps the same
attempt and fence. Renewal cannot revive an expired lease or extend a deadline
that would not advance. Use the current revision for the next operation.

## Publish the complete output

Send `publish` with the job ID, current expected revision, fence, an `assertions`
array, `usage` and an execution `evidence_ref`. Each assertion contains:

```json
{
  "source": {"record_id": "018f0000-0000-4000-8000-000000000001", "revision": 2},
  "target": {"record_id": "018f0000-0000-4000-8000-000000000002", "revision": 1},
  "kind": "same_entity",
  "valid_from_millis": null,
  "valid_until_millis": null,
  "evidence_ref": "trace://session-42/assertion-1"
}
```

Both endpoints must match exact references in the manifest. The database copies
the extractor's method and model identity and binds every other manifest source
as additional relation evidence. A worker cannot omit inconvenient context from
one output. Model origin remains a declared origin; an inferred causal claim does
not become an observed causal fact.

A publication can contain zero assertions when extraction completed without a
supported relationship. It cannot exceed the job's `max_relations`. All assertions,
canonical relations, adjacency entries and completeness headers, sequential change
events, relation receipts, job state, immutable revision and command receipt commit
in one synchronous WAL batch. An invalid final assertion prevents the entire
publication. Shared endpoints accumulate their index counts and history positions
inside that same batch.

After publication, inspect `job.output_receipts` for exact relation identities.
`published` records a completed publication, not permanent permission to reuse
those relations. Updating, rejecting, deleting or expiring any declared source
can invalidate the outputs while the job and its publication history remain.

## Record failure, uncertainty and cancellation

`usage` is explicitly one of:

```json
{"status": "unknown"}
```

```json
{
  "status": "reported",
  "model_calls": 1,
  "input_tokens": 420,
  "output_tokens": 64,
  "cost_microusd": null
}
```

Reported counts are caller observations, not provider billing verification.
A missing or null cost is unknown. Use exact unsigned 64-bit integers; do not
round large counts through a JavaScript floating-point value. Aggregate unknown
consumption as unknown rather than substituting zero.

| Operation | Required additional fields | Durable result |
| --- | --- | --- |
| `fail` | Current fence, `usage`, `evidence_ref` | Close the attempt as failed; ready if attempts remain, otherwise exhausted |
| `recover_expired` | `evidence_ref` | Close an expired or previous-incarnation attempt with unknown execution and consumption; permit a bounded retry if attempts remain |
| `cancel` | `evidence_ref` | Make a ready/running job terminal; fence publication and retain unknown outcome/usage for an interrupted running attempt |
| `reconcile_usage` | Positive `attempt_number`, reported `usage`, `evidence_ref` | Update a closed attempt's usage report; preserve older reports and uncertainty in immutable history |

All operations include `job_id` and `expected_revision`. Recovery can be driven
by application policy without a human confirmation step. It is explicit because
retrying an unknown external outcome may repeat a provider call or cost. Use the
provider's own idempotency or reconciliation mechanism where available. This
contract guarantees one accepted database publication per job; it does not provide
exactly-once external inference.

Restarting or restoring storage changes its incarnation, immediately fencing prior
execution leases. Durable jobs and historical receipts survive. Canceling a job
also fences publication, but does not prove that a remote provider or process
stopped. An executor must separately implement cancellation if supported.

A usage reconciliation does not reopen a job, change its output or rewrite an
unknown execution outcome as success. It records additional consumption evidence.
Failures, corrections and unknown outcomes remain inspectable by revision.

## Discovery, history and limits

Query jobs with `query.limit` (1–100), `query.scan_limit` (1–1,000), optional
`query.status` and optional `query.after`. The cursor is a scanned UUID in the
current owner partition. Filtered jobs still advance it. Follow `next_after` until
null; an empty filtered page can still have a continuation. Pages are not a frozen
snapshot across requests. Rescan from the beginning to discover newly inserted or
changed work that sorts before a previous cursor.

Read `/revision` with `contract_version`, `scope`, `job_id` and a positive
`revision`. It returns that exact `history.job` and `history.receipt`. Never replace
current job inspection with an old acknowledgement when deciding whether to run.

| Boundary | Contract |
| --- | --- |
| HTTP request | 65,536 bytes on all five routes |
| Source manifest | 2–16 distinct current references in one scope |
| Context validation | One combined walk, depth 8 and 64 unique records; shared dependency cap 4,096 records / 16 MiB |
| Source payloads | 8 MiB of serialized canonical records; creation, claim, context and publication enforce this bound |
| Outputs | 0–16 assertions, constrained by the immutable job limit |
| Attempts | 1–32, constrained by the immutable job limit |
| Lease | 1–900 seconds; half-open expiry at the server's validation time |
| Attempt duration | At least one lease interval, at most 24 hours; renewal cannot exceed it |
| Current job / history | 512 KiB / 1 MiB per stored record |
| Job discovery | At most 1,000 scanned jobs and 4 MiB of canonical job bytes per request; integrity/history/output checks are additional |

Hard integrity, source, context or record-budget failures return an error, not a
partially published result. Discovery provides a continuation when its count or
canonical-byte budget stops a scan. These limits bound work; they are not a total
process-memory or provider-spend guarantee.

All responses use `Cache-Control: no-store`. Handle 400 for invalid input or hard
bounds, 401 for missing/expired/revoked credentials, 403 for missing capabilities or
unauthorized scope, 404 for unavailable owner-scoped jobs/revisions, 409 for changed
revisions, inactive fences, stale sources or idempotency conflicts, 413 for request
size, 500 for encountered storage inconsistency and 503 for occupied admission
slots. A storage or network failure can have an unknown commit outcome: retry the
identical command and key before interpreting it as rejected.

This contract is an engineering prerequisite for the asynchronous consolidation
examined in the [graph research map](../research/graph-memory-evidence.md).
Qualification must separately establish transport, recovery and external worker
behavior. Retrieval comparisons and downstream agent-task evaluation remain
separate gates before promoting any extraction or ranking policy.

## Run an external Python extractor

The Python SDK exposes `ConsolidationClient`, `ExternalConsolidationWorker`,
`SQLiteConsolidationJournal`, `ConsolidationInput`, `ConsolidationResult` and
`ConsolidationStopped`. These classes use only the Python standard library.
Inference and provider credentials remain in the application's extractor.

```python
from qilbeedb import (
    ConsolidationClient,
    ExternalConsolidationWorker,
    SQLiteConsolidationJournal,
)

client = ConsolidationClient(
    api_url,
    api_key,
    tenant_id=company_id,
    subject_id=authenticated_worker_subject,
    scope=authorized_scope,
)
worker = ExternalConsolidationWorker(
    client,
    SQLiteConsolidationJournal("/var/lib/my-agent/consolidation"),
    worker_id="relationship-extractor",
)
result = worker.run_once(job_id, application_extractor)
```

Supply an `application_extractor(input)` callback returning a
`ConsolidationResult(assertions, usage, evidence_ref, failed=False)`. Its input
contains the exact manifest, the selected source records, the attempt identifier
and a `renew()` function. Before the callback, the SDK validates the received
record contract: non-deleted payloads, source identifiers and revisions, author
and provenance fields, review disposition, expiration at the response's evaluation
time, and dependency-work bounds. An invalid context stops before the provider
intent is recorded or the extractor is invoked. These are response-contract
checks; they do not independently establish truth or traverse omitted ancestors.
The worker also requires the returned records to match the saved source manifest.
Job inspection and history also validate the immutable specification, complete
model identity for model inference, attempt numbering and fences, hard deadlines,
usage/outcome consistency, and output receipt ownership. Unknown enum values or
inconsistent lifecycle fields stop the SDK instead of authorizing extraction.
Receipt digests remain server-verified identifiers, not independent client proof.

Renew before the lease expires when necessary; the hard
attempt deadline still applies. A valid result with an empty assertion list
completes the job without inventing a relationship. Report unknown consumption as
`{"status": "unknown"}` when evidence is unavailable.

Use a private, persistent directory on a local POSIX filesystem. One journal holds
an exclusive process lock across the provider call; it is not a distributed lock
or an NFS coordination mechanism. The journal stores intent, output assertions and
receipts, but does not store API keys or the source context payloads. Protect its
contents as application evidence.

Before invoking the callback, the worker durably records that external execution
may begin. If the process dies or the callback fails afterward, a subsequent call
stops with `provider_outcome_unknown`; it does not automatically repeat inference.
Resolve the outcome using provider evidence and the explicit job lifecycle. Do
not delete a journal to make an uncertain attempt appear unstarted.

When output has already been durably prepared, retrying `run_once` sends the same
publication command and idempotency key. It does not regenerate the output. The
returned receipt describes the committed revision; `result["inspection"]` is a
separate current inspection. A network error while obtaining that inspection
still requires reconciliation; it does not undo a committed publication.

## Supervise executions as a company administrator

Company supervision requires `credential_admin` and remains restricted to the
administrator's company. It does not borrow a writer's credential or impersonate
the job owner. Discover authorized workspaces with
`GET /api/v1/company/memory/workspaces`, then select the returned `workspace_id`.
Workspace identifiers are opaque selectors; do not construct them from names.
Responses repeat the selected workspace. Verify the contract version, company,
workspace identifier, project, agent, mission, visibility and private subject
before using the result. Matching only a workspace identifier is insufficient
validation of a response. Directory continuations must advance through unique,
ordered workspace identifiers; retain the last confirmed page after an invalid
response rather than substituting an empty directory.

| Endpoint | Request fields in addition to `contract_version: 1` | Result |
| --- | --- | --- |
| `POST /api/v1/company/memory/consolidation/query` | `workspace_id`, `query` | Current jobs across owners in the selected workspace |
| `POST /api/v1/company/memory/consolidation/inspect` | `workspace_id`, `owner_id`, `job_id` | Current job, source eligibility and lease state |
| `POST /api/v1/company/memory/consolidation/revision` | `workspace_id`, `owner_id`, `job_id`, `revision` | Exact historical job and receipt |
| `POST /api/v1/company/memory/consolidation/cancel` | `workspace_id`, `owner_id`, `job_id`, `expected_revision`, `idempotency_key`, `evidence_ref` | Cancellation receipt identifying the actual administrator |

A query uses `limit`, `scan_limit`, `status` and `after`, with the same count and
canonical-byte limits as owner discovery. Its continuation is an object containing
`owner_id` and `job_id`, not the owner endpoint's single UUID. Reuse the exact
returned object. An entry pairs `owner_id` with `summary`; use both the owner and
job when opening details. Pages are live observations, not a company snapshot.
A completed traversal of one workspace does not establish coverage of every
workspace or every change during traversal.

Within a workspace, entries follow the native index order: the UTF-8 bytes of
JSON-encoded `owner_id`, followed by the UUID bytes. This is not locale-aware
alphabetical order or JavaScript UTF-16 string order. A non-null continuation
identifies the last examined record, which may not match the status filter; it
must advance past the requested cursor and cannot precede the last returned
entry. An empty page can therefore still have a continuation.

Consumers should validate response identity, bounds, unique ordered entries and
cursor progress before accepting a page. Preserve the last confirmed cursor and
previously accepted entries when validation or transport fails, and retry that
same page. Do not turn a malformed continuation into a completed traversal.

```json
{
  "contract_version": 1,
  "workspace_id": "<workspace_id returned by discovery>",
  "query": {
    "limit": 25,
    "scan_limit": 100,
    "status": null,
    "after": null
  }
}
```

Cancellation is an explicit administrative decision with an audit reference. It
fences database publication, but cannot guarantee that an external provider has
stopped or has not charged for work. Inspect current state after cancellation and
retain the receipt for audit.

When reading a historical revision, verify the job identifier, original owner,
requested revision and receipt job/revision together. The receipt's commit time
matches the historical job's modification time. The receipt actor can differ
from the owner for administrative cancellation; do not rewrite that actor as the
worker. A verified historical revision does not establish the current lease,
source eligibility or execution status. Read current inspection separately.

Current inspection reports `source_failure` independently of execution status and
lease state. A ready or running job can have changed, deleted, expired, rejected
or unavailable evidence. Dependency limits or cycles can also prevent completing
the eligibility check. Display that condition to administrators; never infer
source eligibility from `status` or from the existence of an active lease.
A null `source_failure` describes only the inspection's `evaluated_at_millis`:
publication rechecks the source manifest and its dependencies.

If the cancellation response is lost, preserve the complete request and retry its
original idempotency key. The console retains the command in browser local storage,
bound to the signed-in account, company, workspace, owner and job. It contains the
audit reason and request identity, not a session token. Closing a tab preserves
that record; clearing site data, changing browser profiles or losing the device
does not. Keep an independent receipt for operational audit.

The console coordinates submission across tabs using Web Locks and rereads the
saved command under that lock. A confirmed command remains available for exact
receipt verification from another tab. If browser storage or coordination is
unavailable, cancellation is not sent. These browser mechanisms do not establish
provider execution status or replace the server's revision and idempotency checks.

Do not infer rejection from a timeout or use a new key
merely because a response was lost. The server checks committed receipts before
revision conflicts under the mutation lock. An explicit HTTP 409
`revision_conflict` therefore rejects this command without applying it: refresh
current details and let the administrator decide whether to issue a new command.
An `idempotency_conflict` has different meaning and must not be treated as this
permission to replace an uncertain request.

Client revisions must retain their full unsigned 64-bit value. Browser clients
must not round them through JavaScript's ordinary number arithmetic when they
exceed its safe integer range. Historical receipts, current lease observations and
external provider outcomes remain distinct pieces of evidence.
