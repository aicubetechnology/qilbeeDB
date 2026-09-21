# Read a memory evidence graph

Status: **0.12.0 platform contract**. Use this API to inspect the current
evidence behind one or more memories, build a source-linked context bundle, or
render an evidence graph without assembling unrelated read snapshots.

The server traverses the outgoing `derived_from` relations already recorded by
[derived memory commands](derived-memory.md). It returns eligible canonical
records and edges pinned to their exact revisions, under one storage snapshot
and one evaluation clock. It performs no model inference, entity extraction or
graph-based ranking. Embeddings remain external, and existing search scores are
unchanged.

## Choose the authorized route

| Route | Required authority | Resource selection |
| --- | --- | --- |
| `POST /api/v1/memory/graph` | `memory_read` for the requested scope | Company and private subject come from the authenticated credential; send project, agent, mission and visibility |
| `POST /api/v1/company/memory/graph` | `credential_admin` in the company | Send a canonical workspace ID from the company memory directory; administrators can inspect every private subject in their company |

Authorization precedes retrieval admission and memory reads. A company workspace
ID grants no authority by itself. Neither route accepts a caller-selected company
or private subject override. The administrative route does not issue temporary
keys. It returns the same eligible graph contract as the scoped route; use
[retained company inventory](../security/company-memory-administration.md) to
inspect deleted markers or rejected and expired records for administration.

## Read scoped ancestry

```http
POST /api/v1/memory/graph
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
  "query": {
    "root_record_ids": ["018f0000-0000-4000-8000-000000000001"],
    "max_depth": 4,
    "node_limit": 128
  }
}
```

Choose roots from current direct reads or retrieval results. Root UUIDs do not
pin a prior search snapshot: this request re-evaluates their current revisions.
Compare returned revisions with the context your application intended to use.
If a root changed between retrieval and graph reading, the returned current graph
can differ. If it became unavailable, its payload is omitted.

| Query field | Contract |
| --- | --- |
| `root_record_ids` | Required; 1–16 distinct UUIDs, preserving request order |
| `max_depth` | Optional; 0–8, default 4. Roots have depth 0 |
| `node_limit` | Optional; 1–256, default 128. Bounds completed canonical record lookups, including missing or ineligible records |

Unknown fields, duplicate roots and out-of-range values return HTTP 400. The
JSON request body is limited to 65,536 bytes. There is no mode, ranking weight,
incoming-edge lookup or continuation cursor in this contract.

## Interpret the result

A scoped response contains `contract_version`, the authorized `scope`, and a
`graph` object. Administrative responses instead contain `company_id`, the
resolved `workspace`, and the same `graph` object.

| Graph field | Meaning |
| --- | --- |
| `traversal_version` | Always `derived_ancestry_v1`; identifies the server traversal contract |
| `evaluated_at_millis` | One server clock used for every node and dependency in the snapshot |
| `max_depth`, `node_limit` | Effective request limits |
| `roots` | One entry per requested UUID, with an explicit status |
| `nodes` | Deduplicated eligible `MemoryRecord` values with minimum traversal depth from the requested roots |
| `edges` | Directed, revision-bound `derived_from` relations whose source and target both appear in `nodes` |
| `coverage` | Completeness, traversal cut reasons and observed work |

Roots have one of three statuses:

- `included`: its current eligible record appears in `nodes`.
- `unavailable`: the lookup completed, but the record is missing or ineligible.
  The response does not disclose which reason applies.
- `not_examined`: the canonical lookup budget was reached before evaluating this
  requested root. This must not be interpreted as absence.

Traversal is breadth first. Roots enter the queue in request order. Each node's
source UUIDs enter in ascending UUID order. A UUID is discovered once, so shared
ancestors appear once and retain their minimum depth. Nodes follow this traversal
order. Edges follow node order and then ascending target UUID order. Root order
therefore matters when a budget cuts the traversal; preserve it when reproducing
a request.

For each edge, `source` identifies the derived memory and `target` identifies
one of its evidence sources:

```json
{
  "source": {
    "record_id": "018f0000-0000-4000-8000-000000000002",
    "revision": 1
  },
  "target": {
    "record_id": "018f0000-0000-4000-8000-000000000001",
    "revision": 3
  },
  "relation": "derived_from"
}
```

Extraction method, method revision and evidence reference remain in the source
node's `record.derivation`. Authenticated authorship remains in `record.author`.
This relation declares a dependency; it does not assert that an extraction is
correct, that two memories are semantically similar, or that one event caused
another. The API does not synthesize undeclared relationships.

## Coverage and work bounds

`coverage.complete` means every requested root was examined and the ancestry of
the eligible roots is represented completely. It does not mean every root was
available, every memory in the workspace was searched, or every useful piece of
context was found. A graph whose roots are all unavailable can be complete and
empty. Always inspect the root statuses as well as completeness.

`stop_reasons` is empty for complete responses. Otherwise it contains one or both
of `depth_limit` and `node_limit`. Both describe omitted ancestry or unexamined
roots, not a ranking result. Edges with omitted endpoints are not returned.
The full source references remain in a returned record's derivation, so clients
can show that the displayed graph is incomplete. If all required nodes were
explicitly requested as roots, a depth-zero request can still describe a complete
graph.

| Work boundary | Behavior |
| --- | --- |
| Canonical lookups | At most `node_limit`; `coverage.records_examined` includes missing and ineligible records |
| Canonical bytes | At most 8 MiB of serialized records across examined nodes; `coverage.record_bytes` includes unavailable retained records |
| Dependency checks | At most 4096 distinct cached dependency lookups and 16 MiB of dependency record bytes; reported in `coverage.dependency_work` |
| Returned graph | At most 256 nodes and 4096 edges, each node retaining the existing 16-direct-source bound |
| Concurrency | Uses the shared retrieval admission slots through response serialization |

Depth and node cuts return HTTP 200 with incomplete coverage. Canonical-byte or
dependency-budget exhaustion fails the whole request with HTTP 400; no partial
graph is returned. Reduce roots or the node limit before retrying. Reducing
display depth alone does not reduce the complete eligibility checks required for
each returned node. An encountered record/index inconsistency also fails the
whole request, with HTTP 500 `storage_inconsistency`.

Work accounting excludes integrity-index bytes, JSON serialization overhead and
temporary allocations. RocksDB can fetch the value that crosses a byte bound
before its size is known. These are decoded-record budgets, not process-memory
or network response-size guarantees. A dependency can also be fetched as a graph
node; those two work counters describe different read paths and can overlap.

## Keep context current

Every returned node must pass the existing transitive eligibility checks, even
when its sources fall beyond the requested display depth. Source revision changes,
review rejection, deletion and expiration can invalidate an entire derived root.
The response then omits that root's payload instead of serving a stale or partly
validated conclusion.

The snapshot is an observation, not a lease. Writes or expiration after evaluation
can make a returned graph obsolete. Revalidate immediately before reuse using a
new graph read or the [current-record batch API](memory-batch-read.md). The
[change feed](memory-changes.md) helps reconcile caches, but expiration can occur
without an event. No cross-request snapshot or cursor is retained by this API.

Graph reading does not mutate memory, reviews, embeddings or checkpoints. A
successful scoped request follows the normal automatic agent-registration
contract. Administrative reads do not register agents.

## Read as a company administrator

Obtain a workspace ID from `GET /api/v1/company/memory/workspaces`, then send:

```http
POST /api/v1/company/memory/graph
Authorization: Bearer <company-admin-session-or-api-key>
Content-Type: application/json
```

```json
{
  "contract_version": 1,
  "workspace_id": "0000000000000000000000000000000000000000000000000000000000000000",
  "query": {
    "root_record_ids": ["018f0000-0000-4000-8000-000000000001"],
    "max_depth": 4,
    "node_limit": 128
  }
}
```

Replace the placeholder workspace ID with a directory result. Missing workspaces
and workspaces from another company share HTTP 404 `record_not_found`. Inside an
existing authorized workspace, unavailable roots use the ordinary root status
instead of returning 404. A valid company administrator can inspect a private
workspace after the original writer's key has been revoked; this administrative
authority does not grant an ordinary agent access to another private subject.

## Errors and integration checks

All responses use `Cache-Control: no-store`. Missing, expired or revoked
credentials return 401; missing capability or scope returns 403. The company
route uses 404 only for an unavailable workspace. Oversized bodies return 413.
When all retrieval slots are occupied, 503 `retrieval_busy` is returned before
traversal; retry with bounded backoff. Do not reinterpret a failed graph request
as a complete empty result.

Real HTTP tests validate responses against the served OpenAPI, including bounds,
root statuses, exact source revisions, two companies, private subjects, resource
scope denials, revoked keys, administrative access, malformed requests and shared
admission. Storage tests cover diamonds, deterministic ordering, snapshot
consistency, changed/rejected/deleted/expired sources, byte exhaustion and corrupt
dependencies. A process-kill test checks graph reconstruction from acknowledged
memory writes and revalidation after a later source edit.

This is evidence traversal, not a retrieval-quality experiment. See the
[graph research map](../research/graph-memory-evidence.md) for typed relations,
graph-assisted ranking and controlled agent-task evaluations still required.
