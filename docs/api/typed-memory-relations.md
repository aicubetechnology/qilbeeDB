# Typed memory relations

Status: **0.13.0 contract**. Record a directed assertion between two
current memories, retain its provenance and review history, and revalidate both
endpoints before serving it. This API is the durable foundation for typed memory
graphs. It does not expand search or extract relations. Use the
[typed graph API](typed-memory-graph.md) to enumerate a bounded neighborhood.

For a graph of existing evidence dependencies, use the separate
[memory evidence graph API](memory-evidence-graph.md). Typed assertions do not
replace `derived_from`, change memory payloads or alter cosine, BM25 or hybrid
scores. External services perform inference and supply model identities; no
provider credentials or embedding vectors are accepted by these routes.

## Select a relation type

Every assertion is directed from `source` to `target`, even when a client regards
the underlying concept as symmetric. The database does not create a reverse edge.
Both endpoints identify memories, not standalone entity nodes.

| `kind` | Meaning |
| --- | --- |
| `semantic_related` | The reporter asserts a semantic connection; no similarity score is implied |
| `same_entity` | The reporter asserts that the memories refer to the same entity; identity resolution is not performed by the database |
| `temporal_before` | The source's recorded event time must be strictly earlier than the target's; insertion order is irrelevant |
| `causal_claim` | The reporter proposes a causal connection; it is not a verified causal fact |
| `supports` | The reporter asserts that the source supports the target |
| `contradicts` | The reporter asserts that the source conflicts with the target |

The reporter's `origin` is one of `model_inference`, `tool_observation`,
`human_statement` or `imported_assertion`. It is a declaration by the caller,
not proof that a human, model or tool actually made the observation. Authenticated
authorship is recorded separately as `reported_by`. An authorized approval is a
review decision, not independent verification of truth or causality.

## Authorize the request

All four routes use POST, `Authorization: Bearer <scoped-api-key>` and
`Content-Type: application/json`. Each request contains `contract_version: 1`
and the existing [resource scope](../security/scoped-credentials.md).

| Route | Required capabilities | Result |
| --- | --- | --- |
| `/api/v1/memory/relations/commands` with `assert`, `retire` or `restore` | `memory_read` and `memory_write` | Durable command receipt |
| `/api/v1/memory/relations/commands` with `review` | `memory_read` and `memory_review` | Durable review receipt |
| `/api/v1/memory/relations/read` | `memory_read` | Current eligible relation |
| `/api/v1/memory/relations/inspect` | `memory_read` and `memory_review` | Retained current relation and eligibility explanation |
| `/api/v1/memory/relations/revision` | `memory_read` and `memory_review` | Immutable historical relation and its receipt |

The credential determines the company and, for private memory, the subject.
Both endpoints and the relation must belong to the exact same authorized
company, project, agent, mission and visibility partition. A UUID conveys no
authority. Requests cannot override the company or private subject. Private
assertions remain private to their subject; authorized shared-scope readers can
read eligible assertions reported by another subject.

Review authority is independent of ordinary write authority. A review-only
credential cannot create, retire or restore a relation. A writer without review
authority cannot inspect retained metadata or historical versions. Native
[company relation inspection and history](typed-memory-graph.md) use separate
company-administrative authority, including all private subjects in the company.

## Assert a relationship

First read the two memories and retain their exact integer revisions. The server
rechecks those revisions and their complete transitive eligibility in the same
write critical section used by memory mutations. A late worker cannot publish a
relation against a source changed, rejected, deleted or expired in the meantime.

```http
POST /api/v1/memory/relations/commands
Authorization: Bearer <scoped-api-key>
Content-Type: application/json
```

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "mission_id": null,
    "agent_id": "agent",
    "visibility": "private"
  },
  "idempotency_key": "extract-run-42-relation-1",
  "operation": {
    "type": "assert",
    "relation": {
      "source": {
        "record_id": "018f0000-0000-4000-8000-000000000001",
        "revision": 2
      },
      "target": {
        "record_id": "018f0000-0000-4000-8000-000000000002",
        "revision": 1
      },
      "kind": "supports",
      "provenance": {
        "origin": "model_inference",
        "method": "support-extraction",
        "method_revision": "prompt-v3",
        "evidence_ref": "trace://run-42/relation-1",
        "model": {
          "provider": "example-provider",
          "model": "example-model",
          "revision": "example-pinned-revision"
        }
      },
      "valid_from_millis": null,
      "valid_until_millis": null
    }
  }
}
```

Replace the fixture IDs and model identity with actual values. Model inference
requires a non-null model with `provider`, `model` and `revision`. Other origins
may omit `model`; the database never assigns a model by assumption. The extractor
identity is distinct from any embedding-space identity attached to the memories.

The endpoints must be distinct UUIDs with positive revisions. Self-relations,
unknown types, unknown fields and caller-supplied confidence or ranking weights
are rejected. Method, method revision and model fields contain 1–256 UTF-8 bytes;
evidence references contain 1–2048 bytes. These strings cannot be blank or contain
control characters. Evidence references are identifiers, not URLs fetched by the
database. Do not put secrets or sensitive source content in them.

Optional validity timestamps use Unix milliseconds. When both are supplied,
`valid_from_millis` must be strictly below `valid_until_millis`. The interval is
half-open: valid at the lower bound and invalid at the upper bound. Future and
historical assertions can be stored but are not eligible outside that interval.
Timestamp validity does not replace endpoint checks.

## Receipts and retries

A successful command returns `contract_version` and `receipt`. The receipt has:

- The server-generated `relation_id`, its positive `revision`, and the original
  `idempotency_key`.
- `action`: `asserted`, `retired`, `restored` or `reviewed`.
- Authenticated `author`, `committed_at_millis` and the operation's `evidence_ref`.
- `relation_digest`, `command_digest` and `receipt_digest`: opaque lowercase
  hexadecimal integrity values. They are not signatures or evidence of truth.

The canonical relation, both adjacency indexes, immutable revision, integrity
metadata, adjacency completeness headers and idempotency receipt are committed
in one synchronous WAL-backed
RocksDB batch. Acknowledged state survives reopening and the tested abrupt process
termination. Storage failures must not be interpreted as a known rejection;
retry the identical command with the same key to resolve an uncertain outcome.

An idempotency key contains 1–256 nonblank UTF-8 bytes without control characters.
It is scoped by the authorized memory partition and authenticated subject.
Credential rotation for the same subject preserves replay. Reusing a key for a
different command returns 409 `idempotency_conflict`. A replay returns the exact
original receipt, including its original author, after verifying its retained
history. It does not establish that the relation is currently eligible.

Use a distinct key for each intended lifecycle change. Repeated assertions with
different keys can create different relation IDs. They are not deduplicated into
independent evidence, and their count must not be treated as corroboration.

## Read and inspect

Send this body to `/read` or `/inspect`:

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "mission_id": null,
    "agent_id": "agent",
    "visibility": "private"
  },
  "relation_id": "018f0000-0000-4000-8000-000000000003"
}
```

`/read` returns `contract_version`, `scope` and `relation`. A relation contains
its ID, revision, immutable `input`, original `reported_by`, creation and
modification timestamps, `state` (`active` or `retired`) and optional `review`.
An active, unreviewed claim can be eligible. Eligibility is permission to serve
this declared assertion, not certification that it is true.

An absent or ineligible relation returns 404 `record_not_found` without disclosing
the reason. Current endpoint revision changes, rejected reviews, deletion,
expiration and invalid transitive evidence suppress ordinary relation reads.
No endpoint payload is included in a relation response. The separate typed graph
response includes eligible canonical endpoint records and bounded assertions.

`/inspect` returns `inspection.relation` and `inspection.eligibility`. It preserves
retired or rejected metadata for authorized reviewers. The explanation contains
`eligible`, `reason`, `evaluated_at_millis`, an optional failing `endpoint`, and
`dependency_work`. Reasons are checked in this order:

1. `retired` or `rejected`.
2. `not_yet_valid` or `expired` for the relation's interval.
3. `endpoint_unavailable` or `endpoint_revision_changed`, checking source then
   target and stopping at the first failure.
4. `eligible` if all checks succeed.

Inspection is a bounded diagnostic, not an exhaustive list of failures. Use the
existing [memory eligibility diagnostic](derived-memory.md) to investigate an
endpoint's ancestry when authorized. Failed byte or dependency checks return an
error, never an eligible or partially validated relation.

## Retire, restore and review

Keep the same request envelope and send an operation such as:

```json
{
  "type": "retire",
  "relation_id": "018f0000-0000-4000-8000-000000000003",
  "expected_revision": 1,
  "evidence_ref": "review://case-42/withdrawal"
}
```

`retire` changes an active relation to retired and removes its serving adjacency
entries. `restore` requires a retired relation, rechecks the exact original
endpoints, and makes it active. Restoring never removes a rejection or rewrites
the immutable input. To correct endpoints, type, model identity or validity,
retire the old assertion and create a new one with new evidence.

For a review, use `type: "review"` and additionally provide `disposition` as
`approved` or `rejected`. Review uses the same expected-revision guard and evidence
reference. Each accepted lifecycle operation advances the relation revision by
one. A rejected review removes serving adjacency entries. Approval cannot make
stale endpoints eligible, undo retirement, or override time validity.

Concurrent changes with the same `expected_revision` produce at most one accepted
new revision. Other distinct commands return 409 `revision_conflict`. Read the
current state and decide whether a new command is still appropriate; do not
blindly overwrite the new revision. Requests that retire an already retired
relation or restore an active one return 400.

To read an immutable version, send the ordinary read body with a positive
`revision` to `/api/v1/memory/relations/revision`. The result contains `history`
with that revision's `relation` and `receipt`. Historical reads do not apply
current serving eligibility. Missing revisions return 404. Revisions are unsigned
64-bit integers; clients must preserve their precision above JavaScript's safe
integer range rather than rounding through a floating-point number.

## Work bounds and cache consistency

| Boundary | Contract |
| --- | --- |
| HTTP body | At most 65,536 bytes on all four routes |
| Relation metadata | Canonical value and each immutable history value are independently bounded to 16 KiB |
| Endpoint records | At most 8 MiB combined serialized canonical record bytes |
| Endpoint eligibility | Existing transitive depth 8 and 64-source-node rules apply independently to each endpoint; the relation adds no derivation depth |
| Shared dependency work | At most 4096 distinct dependency lookups and 16 MiB per request; reported by inspection |
| Admission | All routes use the shared retrieval slots after authorization, through response serialization |

These are record-work bounds, not a process-memory or total response-size
guarantee. Integrity-index reads, serialization overhead and temporary allocations
are additional. RocksDB can fetch the value that crosses a byte bound before its
size is known. Exhaustion fails the whole request with 400; it is not a complete
empty result. Reduce the size or dependency footprint of the endpoints before
retrying. There is no unbounded traversal option.

Reads use one snapshot and clock, but a successful read is not a lease. Re-read
the relation immediately before reuse; endpoints can change and validity can
expire after the response. The [relation change feed](typed-relation-changes.md)
now emits typed lifecycle events with history-bound cursors and subject-owned
checkpoints. It is separate from memory changes: observe both streams and retain
current-state validation. A memory checkpoint alone cannot certify a relation
cache as current. Bounded neighbor enumeration is available through the typed
graph API; automatic extraction and consolidation workers remain separate work.

## Errors and validation

All responses use `Cache-Control: no-store` and the published platform error
envelope. Handle errors explicitly:

| HTTP status | Common code or cause |
| --- | --- |
| 400 | Invalid version, malformed input, invalid lifecycle state, temporal ordering or bounded-work exhaustion |
| 401 | Missing, expired or revoked credential |
| 403 | Missing capability or unauthorized scope |
| 404 `record_not_found` | Relation unavailable for ordinary read, or absent retained relation/history in an authorized inspection |
| 409 `revision_conflict` | Stale relation revision or an endpoint no longer current and eligible |
| 409 `idempotency_conflict` | Changed command under an already committed key |
| 413 | Oversized request body |
| 500 `storage_inconsistency` | Encountered relation, receipt, history or index corruption |
| 503 `retrieval_busy` | Shared work slots occupied; use bounded backoff with the same command key |

Qualification covers actual HTTP responses against the served OpenAPI, private
subjects, companies, authorized and unauthorized resource partitions, revoked
credentials, capability separation, byte and concurrency bounds, historical
integrity, competing commands, eventless expiry and process-kill recovery. These
tests use controlled fixtures. They do not establish search relevance, independent
corroboration, agent-task improvement or production throughput. The
[graph research map](../research/graph-memory-evidence.md) records the remaining
traversal, retrieval and evaluation requirements.
