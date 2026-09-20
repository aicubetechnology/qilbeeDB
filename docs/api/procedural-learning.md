# Procedural Learning API

For structured candidates derived from exact experience observations, use the
[experience strategy API](experience-strategies.md), available in 0.10.0. It
registers candidates through this authority and contributes no qualification
trials by itself.

The authenticated `/api/v1/learning` API registers immutable experimental
contracts, proposes candidates, records observed outcomes and selects qualified
procedures. The same durable learning ledger owns qualification and monitoring
decisions. Clients submit requests and receive results; these endpoints do not
run models, tools or arbitrary code.

Use `http://localhost:7474` from the browser or a client on the Docker host.
Import [OpenAPI](openapi.json) for complete request and response schemas.
All JSON requests require `contract_version: 1`, use the existing 64 KiB transport
limit, and reject unrecognized fields. Responses carry `contract_version: 1`
and `Cache-Control: no-store`.

## Authentication and scope

Use `Authorization: Bearer <credential>`. Tenant and actor derive from the live
credential. Scoped operations use the same exact `scope` as versioned memory:

```json
{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"}
```

Shared access requires an exact grant within the tenant. Private access includes
the authenticated subject, so another subject's private namespace is distinct.
A separate evaluator subject needs a shared scope to evaluate another subject's
proposal. Credential capability names grant no implicit additional permissions.

| Method and endpoint | Capability | Result envelope |
| --- | --- | --- |
| `POST /api/v1/learning/policies` | `policy_admin` | `entry`: immutable policy registration |
| `GET /api/v1/learning/policies/{id}` | `policy_admin` | `entry`: registered policy |
| `POST /api/v1/learning/contexts` | `policy_admin` | `entry`: immutable context registration |
| `GET /api/v1/learning/contexts/{id}` | `policy_admin` | `entry`: registered context |
| `POST /api/v1/learning/proposals` | `procedure_propose` plus scope grant | `receipt`: original candidate registration |
| `POST /api/v1/learning/procedures/read` | `memory_read` plus scope grant | `procedure`: original receipt and current record |
| `POST /api/v1/learning/evaluations` | `procedure_evaluate` plus scope grant and policy evaluator subject | `receipt`: durable admission outcome |
| `POST /api/v1/learning/evaluations/read` | `memory_read` plus scope grant | `receipt`: original admission |
| `POST /api/v1/learning/select` | `memory_read` plus scope grant | `selection`: qualified procedure or exact baseline |

Successful operations return **200**, including identical retries and durable
admission of rejected/incomplete evidence. HTTP success alone does not mean an
evaluation was accepted or a procedure was promoted.

## Register the administrative contracts

A policy administrator posts this to `/api/v1/learning/policies`:

```json
{
  "contract_version": 1,
  "id": "policy-v1",
  "definition": {
    "algorithm": "fixed_budget_hoeffding_v1",
    "parameters": {
      "qualification_trials": 32,
      "confidence_delta": 0.01,
      "min_improvement": 0.05,
      "min_candidate_utility": 0.7,
      "max_cost_units": 10000,
      "max_latency_ms": 60000,
      "max_failure_streak": 3,
      "evaluator_id": "evaluator",
      "evaluation_contract": "rubric-v1"
    }
  }
}
```

The evaluator ID is an authenticated **subject**, not a client-supplied actor
field. The algorithm uses a fixed qualification count without early acceptance;
its statistical assumptions and per-proposal error budget are documented in
[the learning contract](../agent-memory/learning.md). This example is suitable
for a synthetic integration test, not a claim of measured agent improvement.

Register the exact context at `/api/v1/learning/contexts`:

```json
{
  "contract_version": 1,
  "id": "context-v1",
  "context": {
    "task": "task-v1",
    "baseline_revision": "baseline-v1",
    "model_provider": "provider",
    "model_revision": "model-v1",
    "tools": {"search": "search-artifact-v1"},
    "environment_revision": "environment-v1",
    "evaluation_contract": "rubric-v1",
    "dataset_revision": "held-out-v1",
    "harness_revision": "harness-v1",
    "permissions_revision": "permissions-v1"
  }
}
```

Policy/context IDs are immutable within a tenant. The response entry preserves
`schema_version`, `tenant`, `id`, `payload`, `payload_digest`, `registered_by` and
`recorded_at_millis`. The actor string is `<credential UUID>:<subject ID>`.
An identical retry returns the original entry; changed content returns 409.
A new revision needs a new ID. Registry identities support 1–512 UTF-8 bytes,
and a context supports up to 128 tool identities.

Definitions are tenant-wide administrative registrations. Authorized proposers
may reference registered definitions; this version does not implement automatic
retirement or a mutable per-scope active-policy assignment. Administrators must
register only definitions they intend to make available in that tenant.

## Propose an immutable candidate

Post to `/api/v1/learning/proposals` using a proposing credential:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "proposal": {
    "id": "candidate-v1",
    "policy_id": "policy-v1",
    "context_id": "context-v1",
    "instructions": "Check source evidence before answering.",
    "source_refs": ["training-trace-v1"]
  }
}
```

Task, baseline and policy parameters are resolved from the registered contracts.
The caller cannot insert policy thresholds into this request. Source references
number 1–128, each 1–2048 bytes; instructions support up to 65536 bytes in the
library, subject to the smaller effective space available in the whole HTTP body.
References declare provenance; the platform does not fetch or authenticate
arbitrary external evidence merely because its ID appears here.

The `ProposalReceipt` preserves tenant, authorized namespace, original request,
policy/context digests, authenticated actor and original `Candidate` record.
Candidate and receipt commit in one synchronous WAL batch. The procedure ID is
unique within tenant/namespace; changed retries conflict. Identical retries always
return this original receipt, even after later promotion or suspension.

To read current state, post to `/api/v1/learning/procedures/read`:

```json
{"contract_version":1,"scope":{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},"procedure_id":"candidate-v1"}
```

The `procedure` envelope has `receipt` (original registration) and `record`
(current `ProcedureRecord`). The latter includes immutable proposal/scope,
`state`, qualification and monitoring counts, mean improvement/utility, budget
violations, lower improvement bound, failure streak, creation time and decision
history. Each decision records its prior/next state, reason, triggering case and
transaction timestamp. States are `Candidate`, `Active`, `Rejected`, `Suspended`.

## Submit outcomes with an evaluator credential

Post to `/api/v1/learning/evaluations`:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "procedure_id": "candidate-v1",
  "submission": {
    "case_id": "held-out-case-001",
    "phase": "Qualification",
    "policy_id": "policy-v1",
    "context_id": "context-v1",
    "baseline_revision": "baseline-v1",
    "evidence_ref": "evaluation-trace-001",
    "status": "complete",
    "baseline_utility": 0.0,
    "candidate_utility": 1.0,
    "candidate_cost_units": 100,
    "candidate_latency_ms": 250,
    "detail": null
  }
}
```

Baseline and candidate must describe the same held-out case. Valid utilities lie
in `[0,1]`. Cost uses the unit defined by the evaluation contract; latency uses
milliseconds. Measurements can be null/omitted to express unknown values.
Status is `complete`, `rejected`, `incomplete`, `cancelled` or
`pending_or_unknown`. Phase is `Qualification` or `Monitoring`.

The `AdmissionReceipt` retains schema, tenant/namespace/procedure, authenticated
`actor` (`subject_id`, `credential_id`), exact `submission`, `outcome`, stable
`reason`, `state_after`, optional accepted `evaluation` and transaction time.
Only `outcome: accepted` updates the efficacy ledger. Other outcomes are
`rejected`, `incomplete`, `cancelled`, `pending_or_unknown`,
`unknown_consumption`; see [admission semantics](../agent-memory/evaluation-admission.md).
An accepted observation may be negative and cause statistical rejection or
suspension. Missing accounting never becomes zero.

Case IDs are immutable across phases and outcomes. Identical retries by the same
evaluator subject preserve the original receipt; changed submissions return 409.
Corrected experiments require new case IDs and cannot reuse already accepted
evidence as independent evidence. To read a receipt, post the procedure-read
request plus `case_id` to `/api/v1/learning/evaluations/read`.

## Select or fall back to the exact baseline

Post to `/api/v1/learning/select`:

```json
{"contract_version":1,"scope":{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},"policy_id":"policy-v1","context_id":"context-v1","max_instruction_bytes":4096}
```

The instruction budget is 0–65536 **UTF-8 bytes**, not model tokens. Selection
requires an active procedure qualified under those exact registered IDs/digests.
It scans that learning scope under the mutation lock, checks the selected record
against its immutable binding, and ranks by lower improvement bound with a stable
procedure-ID tie break. No indexed-scale latency claim is implied.

A qualified response has `selection.type: procedure` and a `procedure` containing
its original receipt and current record. Otherwise the complete response is:

```json
{"contract_version":1,"policy_id":"policy-v1","context_id":"context-v1","selection":{"type":"baseline","baseline_revision":"baseline-v1","reason":"no_qualified_compatible_procedure"}}
```

Changing model, tools, environment, permissions or another context field requires
a new context and fresh qualification. The old procedure is not silently reused.
Monitoring regressions can suspend an active procedure and restore baseline
selection. This is the platform's single current selection result, not an
execution permit or a guarantee that a selected procedure cannot regress later.

## Failure and recovery contracts

| Status | Contract |
| --- | --- |
| 400 | Invalid shape/version, invalid registration or incompatible policy/context proposal |
| 401 | Missing, expired, rotated or revoked credential |
| 403 | Missing capability/grant or wrong authenticated evaluator subject |
| 404 | No referenced contract, procedure or receipt in the authorized namespace |
| 409 | Immutable ID/case reused with changed content |
| 413 | Request exceeds the transport body limit |
| 500 | Storage failure or inconsistent/unsupported persisted record |

A well-formed evaluation with mismatched contract or unusable evidence returns a
200 **rejected admission receipt**, rather than silently counting evidence.
Authentication failures and malformed requests do not create admission records.
Synchronous WAL preserves acknowledged ledger mutations across process crashes;
accepted admission and decision changes are atomic. An authorized in-flight
request may finish during concurrent credential revocation. Unsupported schema
or inconsistent binding/evaluation data fails explicitly. Deletion of procedural
history, policy retirement, external trace verification and remote execution are
separate capabilities outside this contract.
