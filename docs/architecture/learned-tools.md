# Learned tools as platform services

## Accepted architecture

QilbeeDB will own the service contracts for developing, repairing, versioning,
evaluating, publishing, discovering and invoking learned tools. Executors remain
isolated on servers and are reached through the platform. User devices keep a
light client for authentication, requests, status and results. QMN and other
applications consume the same platform contracts.

This is an accepted direction and an implementation acceptance contract, **not a
claim that learned-tool endpoints already exist**. The currently implemented
HTTP surface is listed in [OpenAPI](../api/openapi.json). Local Docker is the
integration environment requested for validation; production placement can use
remote database and executor servers without moving execution onto user devices.

## Service boundaries

| Component | Responsibility | Durable authority |
| --- | --- | --- |
| QilbeeDB artifact service | Store source, schemas, dependencies, content digest and parent/repair provenance | Immutable artifact revision and lineage |
| QilbeeDB development service | Receive development/repair requests; coordinate configured generation and testing services | Request, candidate, captured failure and cancellation history |
| QilbeeDB evaluation/publication service | Register comparisons, validate context/evidence, decide releases and rollback | One policy-versioned decision and exact approved artifact |
| QilbeeDB execution gateway | Authorize invocation, reserve idempotent identity, dispatch, query and cancel | Durable invocation receipt and current known state |
| Remote isolated executors | Run an exact artifact under configured permissions and resource limits | Execution evidence returned under authenticated worker identity |
| Client SDK or QMN adapter | Propose, discover, invoke and render status/results | No independent publication decision or hidden execution authority |

The database process does not import learned Python modules, spawn arbitrary
learned programs, or share its filesystem/credentials with their runtime. A
gateway and development worker can be independently deployed server services
under the QilbeeDB API and authority. Absorption does not require running every
service in the same process or container.

## Artifact and evidence contracts

Every candidate must identify tenant, authorized scope, immutable source digest,
input/output schemas, runtime/dependency identity, originating request, parent
revision and applicable policy. A repair creates a new artifact and preserves
its failure evidence; it never silently edits a published version.

Evaluation binds the exact baseline, candidate, tasks, model/provider, tools,
environment, evaluator/harness, permissions and executor image. Reject unknown
policy versions and mismatched references. Incomplete, rejected, cancelled and
unknown-consumption outcomes remain distinguishable records. Unknown token or
resource use cannot become zero cost or positive efficacy evidence.

A published tool references exactly one accepted decision under a supported
policy. A rollback changes the release pointer to a specific previously approved
revision and preserves history. Proposing, evaluating, publishing, administering
policy and invoking are separately authorized actions. Capability names and
HTTP routes for these actions will be added with their implementations and tests;
they are not advertised as working endpoints in the current OpenAPI document.

## Invocation and uncertainty

An invocation binds tenant, scope, caller, invocation ID, exact release revision,
artifact digest, input digest and execution policy. The gateway persists its
reservation before dispatch. Identical retries return the same operation state;
a changed request under the same invocation ID conflicts.

A network timeout or gateway restart can leave execution outcome unknown. In
that case the API must expose `pending_or_unknown` or an equally explicit state,
keep resource use unknown when unverified, and avoid automatic redispatch that
could duplicate external effects. Reconciliation queries the original worker
operation. A cancellation request is not proof that execution stopped; terminal
cancellation requires evidence from the executor or an explicit policy outcome.

Composition binds exact component artifacts and records each child receipt and
failure. Unknown or cancelled child execution cannot silently become aggregate
success. Tool availability and efficacy do not grant execution permissions.

## Reference evidence from QMN

Read-only inspection at `qilbee-ecosystem` revision
`66cf421f68fa380aeffccb8fa84d8cfbeca3f5c6` found these concrete contracts:

- The [program gateway](https://github.com/aicubetechnology/qilbee-ecosystem/blob/66cf421f68fa380aeffccb8fa84d8cfbeca3f5c6/services/qmn/services/shared/program_gateway.py)
  binds tenant/project/invocation identity to an expected release and artifact,
  persists reservation and exposes unknown state without automatic redispatch.
- The [worker transport](https://github.com/aicubetechnology/qilbee-ecosystem/blob/66cf421f68fa380aeffccb8fa84d8cfbeca3f5c6/services/qmn/services/shared/program_transport.py)
  keeps worker credentials server-side, validates private credential files,
  constrains requests/responses and represents uncertain execution explicitly.
- The [program evidence gate](https://github.com/aicubetechnology/qilbee-ecosystem/blob/66cf421f68fa380aeffccb8fa84d8cfbeca3f5c6/services/qmn/services/shared/program_experiments.py)
  rejects invalid program/composition evidence and clears efficacy statistics
  when the required execution proof is missing.

These are source observations, not a claim that the complete QMN suite was run.
Migration must preserve the observable contracts through QilbeeDB-owned tests,
without retaining a mandatory QMN service or database connection.

## Delivery and validation sequence

1. Expose the procedural evaluation/publication contract with exact compatibility
   and one decision authority, including rejected and incomplete evidence.
2. Add the immutable artifact registry and development/repair receipts, with
   provenance and separate credentials for development services.
3. Add remote executor registration and invocation/status/cancel contracts.
4. Validate paired program evidence, composition, publication and rollback.
5. Run end-to-end tests from a lightweight client against the local Docker
   platform and an independently isolated test executor.

Acceptance includes cross-tenant denial, duplicate invocation prevention,
changed-input conflicts, wrong-artifact and wrong-environment rejection,
executor failure, gateway crash after dispatch, cancellation races, unavailable
accounting, replay after restart, and rollback to an exact artifact. The client
must remain usable without a compiler, local model, container daemon or local
executor. The platform must start and serve its supported APIs with QMN absent.
