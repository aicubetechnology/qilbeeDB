# Agent memory delivery acceptance contract

Maintainer requirements recorded on September 20, 2026. This contract takes
priority over speculative research features. Status must be supported by
reproducible tests against a published commit.

## Platform independence and decision authority

QilbeeDB absorbs capabilities, not a runtime dependency on QMN. It must start,
persist, retrieve, authorize and evaluate procedures without QMN services or
clients installed. Its public API, durable schemas, credentials, policies and
decision authority are platform contracts usable by unrelated applications.
Qilbee, Command Center and QMN are clients/integration targets, not prerequisites.

Inspect QMN service code and its native client to identify observable contracts
and migration requirements. Preserve identifiers and semantics through explicit
adapters where needed; do not import QMN service dependencies into the database.
Promotion and suspension must have one authoritative decision history. Rejected,
incomplete and unknown-consumption outcomes cannot be equated with successful,
fully measured evaluation merely because operation names match.

Capabilities to map include memory/search/tags/review/deletion; authorized
knowledge distribution and context collection; agent identity and graph;
observed outcomes and relationship feedback; scoped session/mission discoveries;
experience/procedures/consolidation; paired experiments and approved releases;
immutable learned programs, repair provenance and cancellation; isolated remote
execution receipts, unknown states and composition; administrative learning
policy; and credential rotation/revocation. Each requires implementation evidence,
contract tests and a migration decision. Historical README claims are not proof.

## P0: first usable integration

### 1. Durable, versioned and idempotent memory API

Connect HTTP episode operations to durable storage. Expose create, update, read,
query and delete with a contract version and idempotency identifier. Replaying
the same request returns its original receipt; changing its content under the
same identifier is rejected. Record/index version mismatches must be explicit.

Acceptance: write and acknowledge through HTTP, terminate the server abruptly,
restart, and recover every acknowledged record without logical duplicates.
Graceful reopen alone does not satisfy this criterion.

### 2. Authenticated tenant and sharing scopes

Derive the tenant from the authenticated credential. Enforce user, project,
mission and agent scope on reads, writes, retrieval and events. Sharing is
explicit. Separate proposal, evaluation and policy administration capabilities.
Client-controlled JSON fields are not an authorization authority.

Acceptance: two tenants can use identical project and agent identifiers without
reading or changing each other's data. Semantic retrieval and event consumers
observe the same boundary. A proposing agent cannot submit evaluations or
change policy with its ordinary credential.

### 3. Stable procedural learning API

Expose `LearningMemory::propose`, `record_evaluation`, `get`, `evaluation` and
`select`. Carry the baseline revision, immutable candidate, model, tools,
environment, evaluator contract, evidence, cost and decision. Expose a single selection authority
through an explicit integration contract usable by QMN or any other client. Do not build a
second agent executor.

Acceptance: candidate, active/rejected and suspended states are reproducible
through HTTP and survive restart with their history. Duplicate evaluations do
not inflate evidence. Contracts and environment versions cannot mix; a model
change does not automatically inherit validated effectiveness. Selection
returns a qualified compatible procedure or an explicit baseline fallback.

## P1: useful, shared and correctable memory

### 4. Provenance, human review and propagated corrections

Distinguish tool observation, user statement, agent output, hypothesis,
synthesis and verified outcome. Record source, revision, event/transaction
times, dependencies, reviewer and current state. Administrative verification
must be reversible. Content erasure differs from invalidation with history.
Execution policy remains administrative configuration.

Acceptance: old agent output cannot become independent evidence; review returns
author, time and scope. Invalidating a source prevents ordinary access to valid
derivatives. Propagation completion is observable so clients can invalidate caches.

### 5. Durable scoped changes for agent collaboration

Expose versioned changes with a durable cursor and reconnection/resume support.
Specify ordering, retention, cursor expiry and recognizable redelivery.

Acceptance: ten test clients exchange mission discoveries; a disconnected
client catches up without silent loss, cross-scope leakage or logical
duplication. These are temporary test clients, not permanent services. Task
reservation and scheduling remain Qilbee/QMN responsibilities.

### 6. Persistent, measurable lexical and hybrid retrieval

Expose lexical/hybrid search with scope, validity, type and revision filters.
Use a lexical index appropriate to the agreed workload. Persist embeddings
with model, dimension and revision; rebuild indexes without indiscriminately
recomputing embeddings. Keep lexical retrieval functional during provider
outages. Bytes and tokenizer-versioned tokens are distinct budgets.

Acceptance: reference results survive restart; incompatible embedding spaces
never mix. Publish quality, latency and memory measurements at a representative
volume, including the environment and methodology.

### 7. Policy-controlled consolidation and retention

The tenant's Command Center/QMN configures versioned policy and incremental
maintenance. Do not impose a mandatory business quota by default; distinguish
optional policy limits from technical transport limits. Preserve protected
evidence, failures and counterexamples according to policy.

Acceptance: maintenance frequency does not change forgetting for the same
elapsed time. Policies are auditable; protected records remain. Corrections and
deletion follow requirement 4. Summarization is not verification and does not
promote a procedure without evaluation.

## P2: qualification for expanded operation

### 8. End-to-end integrity, recovery and metrics

Qualify atomic entity/index changes, concurrency, intermediate failures,
backup/restore and write, retrieval, consolidation, storage and rebuild metrics.

Acceptance: injected failures leave no partial transaction. Restore matches
known records and indexes. Concurrent updates cannot be silently lost; report
conflicts. Publish p50/p95/p99, throughput, memory and disk with hardware,
volume, commit and methodology. Derive capacity/latency goals from the workload.

## Delivery order

Complete **1 + 2 + 3** with API documentation and contract tests first. Then
complete **4 + 5 + 6** for collaboration and corrections. Work on **7 + 8**
alongside that sequence. Integrity necessary for acknowledged-write persistence
belongs to P0 even though scale and broader recovery qualification come later.

Use [validated feature PRs and five-PR batches](../contributing/feature-delivery.md).
The existing library features are foundations, not evidence that these API
acceptance criteria have already been met.

## Implementation checkpoints

The [durable HTTP foundation](../api/durable-http-memory.md) and
[scoped credential authority](../security/scoped-credentials.md) are separate
validated increments. The [platform HTTP API](../api/platform-http.md) adopts durable credentials and
disables legacy routes by default. These checkpoints do not mark the full P0
contracts as complete.
