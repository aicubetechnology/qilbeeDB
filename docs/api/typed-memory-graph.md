# Traverse a typed memory graph

Status: **0.13.0 contract**. Read a bounded neighborhood of
[typed memory assertions](typed-memory-relations.md), with current eligible
memories, exact endpoint revisions, original provenance and explicit coverage.
The server evaluates the graph in one storage snapshot and at one clock value.

Use this contract to inspect connections or assemble graph context from selected
roots. It does not choose search anchors, assign relevance scores, infer entities
or generate relationships. Existing cosine, BM25 and hybrid search are unchanged.
Declared `derived_from` dependencies remain available through the separate
[evidence ancestry API](memory-evidence-graph.md). A complete typed neighborhood
does not imply that all derivation ancestry is displayed.

## Choose the authority and route

| Route | Required authority | Selection |
| --- | --- | --- |
| `POST /api/v1/memory/graph/typed` | `memory_read` | An authorized resource scope and root memory IDs |
| `POST /api/v1/company/memory/graph/typed` | Company `credential_admin` | A workspace from the native company directory and root memory IDs |
| `POST /api/v1/company/memory/relations/inspect` | Company `credential_admin` | Retained current assertion and its eligibility explanation |
| `POST /api/v1/company/memory/relations/revision` | Company `credential_admin` | Immutable assertion version and its receipt |

The scoped route derives the company and private subject from the credential.
Project, agent, mission and visibility are checked before any adjacency selection
or retrieval admission. Every node, relation, request-local cache and counter
belongs to that one authorized namespace. Incoming traversal never grants access
to another scope. A relation UUID or workspace ID conveys no authority.

Native company administration includes all private subjects in the company,
including memories whose original writer's credential has been revoked. It uses
the administrative session or API key directly and issues no delegated agent key.
An ordinary integration credential cannot call the company routes. Company
administration does not grant access to another company.

## Read a neighborhood

```http
POST /api/v1/memory/graph/typed
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
    "direction": "both",
    "relation_kinds": ["supports", "contradicts", "semantic_related"],
    "max_depth": 2,
    "node_limit": 128,
    "edge_limit": 256,
    "scan_limit": 1024
  }
}
```

Replace the fixture root with a real current memory ID. A root ID does not pin
an earlier search revision: the request reads its current canonical record.
Compare returned revisions with the context your application intended to use.

| Query field | Default and permitted values |
| --- | --- |
| `root_record_ids` | Required: 1–16 distinct UUIDs, in caller-selected order |
| `direction` | `both`; also accepts `outgoing` or `incoming` |
| `relation_kinds` | All six kinds; a nonempty list of distinct accepted kinds |
| `max_depth` | 2; range 0–8, with roots at depth zero |
| `node_limit` | 128; range 1–256 completed canonical record lookups |
| `edge_limit` | 256; range 1–1024 eligible assertions returned |
| `scan_limit` | 1024; range 1–4096 physical adjacency entries examined |

All kinds are `semantic_related`, `same_entity`, `temporal_before`, `causal_claim`,
`supports` and `contradicts`. The kind-list order does not affect traversal.
Unknown fields, types, duplicate roots, duplicate kinds and out-of-range limits
return 400. There are no caller-supplied weights or ranking parameters.

Outgoing traversal follows an assertion's source to its target. Incoming traversal
follows the reverse direction for discovery, while preserving the original
assertion's source and target in the response. `both` visits outgoing entries
before incoming entries for each node. It does not create reverse assertions or
turn a causal claim into an observed fact.

## Interpret nodes, edges and roots

The scoped response contains `contract_version`, `scope` and `graph`. The company
graph response contains `contract_version`, `company_id`, the resolved `workspace`
and the same `graph` contract.

| Graph field | Meaning |
| --- | --- |
| `traversal_version` | Always `typed_relations_v1`, the server-owned traversal contract |
| `evaluated_at_millis` | One server evaluation clock for the complete response |
| `direction`, `relation_kinds` | Effective selection parameters |
| `max_depth`, `node_limit`, `edge_limit`, `scan_limit` | Effective work and output limits |
| `roots` | One entry per input root, preserving request order |
| `nodes` | Eligible canonical memory records with their minimum discovery depth |
| `edges` | Current `MemoryRelation` values, retaining relation revision, original direction, declared provenance, reporter, validity and review |
| `coverage` | Completeness, cuts and work counters |

A root is `included`, `unavailable` or `not_examined`. Unavailable means its lookup
completed but no eligible record can be served. The reason is not disclosed.
Not examined means the canonical lookup budget prevented evaluation; it must not
be treated as absence. Roots are read in request order before any neighbors, so
an early expansion cannot consume the budget intended for a later root lookup.
If `node_limit` is smaller than the root count, later roots remain unexamined.

Each returned assertion has both endpoints in `nodes`, at the exact revisions
in `edge.input.source` and `edge.input.target`. Rejected or retired assertions,
invalid assertion intervals, changed endpoint revisions, deleted/expired/rejected
memories and invalid transitive evidence are excluded. Complete memory eligibility
checks apply even when the memory's evidence sources lie beyond the display depth.

The 0.14.0 [additional context extension](typed-memory-relations.md#bind-the-context-used-for-inference)
also checks each assertion's declared `evidence_sources` before expanding or
deferring an edge. Invalid context cannot create a misleading depth cut. Context
IDs and revisions remain in the returned assertion; their payloads are not added
to `nodes`. Reads count toward `coverage.dependency_work`, share its cache and
hard limits, and do not consume the topology `node_limit`. The context walk itself
is bounded to 64 records and depth 8. An unrelated eligible root remains available
even when an assertion that refers to it becomes ineligible.

An unreviewed assertion can be eligible. The original `origin` and model identity
remain caller declarations, while `reported_by` is authenticated authorship.
Approval remains a review decision. Parallel assertions with different relation
IDs are preserved, including repeated claims; their number is not independent
corroboration or a confidence estimate.

Nodes are discovered breadth first. Within each node and index direction,
assertions follow ascending relation UUID order. A memory appears once; an
assertion appears at most once even when reached through both indexes or several
roots. Valid edges between already included nodes can be returned at depth zero.
Edges follow discovery order; a boundary-deferred edge whose endpoints later
appear is appended during final reconciliation in its original encounter order.
Preserve root order, relation IDs and revisions when reproducing a query.

## Distinguish a complete result from a work cut

`coverage.complete` is true only when every requested root was examined and the
selected directed typed neighborhood was fully examined without a work cut.
It does not certify the entire corpus, undiscovered semantic knowledge, evidence
independence or retrieval relevance. All unavailable roots can produce a complete
empty result. A kind filter can also produce a complete root-only graph.

Incomplete responses have one or more `stop_reasons`:

- `depth_limit`: a potentially relevant connection would expand beyond the
  requested depth. Its other endpoint may remain unexamined; the cut does not
  certify that the omitted assertion is eligible.
- `node_limit`: another canonical record could not be examined. This includes
  roots and candidate neighbors, not just nodes that would be returned.
- `edge_limit`: another eligible connection could not be included in the output.
- `scan_limit`: a further scoped adjacency entry exists beyond the work budget.

Filtered, duplicate, expired and stale assertions consume adjacency work, not the
valid edge limit. For example, four deleted neighbors followed by one valid
neighbor can return the valid edge with `edge_limit: 1` when the scan and node
budgets cover all five candidates. A smaller work budget can stop before that
edge, but the response then explicitly reports incomplete coverage. Retired and
rejected assertions have their adjacency entries removed atomically.

There is no continuation cursor or retained cross-request snapshot. A retry with
larger limits is a new observation and can see mutations or expiration. Do not
merge independent responses into an allegedly atomic graph. If an application
requires complete context, reject an incomplete response or apply an explicitly
documented fallback; do not silently present it as a complete empty graph.

## Work, bytes and integrity

| Coverage field | What is counted |
| --- | --- |
| `records_examined` | Completed distinct canonical lookups, including missing and ineligible neighbors; at most `node_limit` |
| `record_bytes` | Canonical memory bytes fetched through those lookups; at most 8 MiB |
| `adjacency_entries_examined` | Physical scoped index entries examined, including duplicates and filtered/stale assertions; at most `scan_limit` |
| `relations_examined` | Distinct canonical assertions loaded; at most the adjacency count |
| `relation_bytes` | Serialized canonical assertion bytes; at most 4 MiB |
| `dependency_work` | Shared transitive validation cache: at most 4096 distinct lookups and 16 MiB |

Node, edge, depth and scan cuts return 200 with incomplete coverage. Byte-budget
or dependency-budget exhaustion fails the whole request with 400; no partial graph
is returned. Each memory retains the existing transitive depth-eight and
64-source-node eligibility limits. Typed discovery depth does not weaken those
checks. All four routes share the configured retrieval admission slots through
response serialization.

Counters are logical storage-work bounds, not an RSS or total network-byte limit.
They exclude integrity headers, relation history, index bytes, JSON overhead and
temporary allocations. RocksDB can fetch a crossing value before its size is
known. A scoped key lookahead identifies a scan cut without decoding an additional
assertion. Lower and upper iterator bounds prevent neighboring scopes from
contributing index candidates or counters.

Every endpoint revision has outgoing and incoming completeness headers. Their
entry counts and digest accumulators are updated in the same synchronous WAL
batch as the assertion, both indexes, history and receipt. New memory revisions
receive empty headers in their own atomic memory mutation batch. A fully scanned
adjacency prefix must match its header; missing entries, including removal of both
directions, cannot silently become a complete empty graph. Each encountered
assertion also verifies its canonical value, both index values and current
immutable history. A corrupt encountered header or value returns 500
`storage_inconsistency` with no partial result.

A cut prefix cannot be fully verified against its aggregate header and remains
incomplete. These checks detect inconsistent storage, not coordinated malicious
rewriting of every checksum or a full-volume corruption audit. Disconnected and
unexamined prefixes are outside the requested observation.

## Upgrade and recovery

Before serving requests, startup initializes missing or stale headers from
canonical memory revisions and the current assertion ledger. It validates the
existing record/index and assertion/history pairs. Source data is not rewritten,
and memory feed positions and command receipts remain unchanged. Rebuild batches
contain at most 256 header rows. A final namespace marker is committed only after
the build succeeds. If the process stops during initialization, reopening clears
that namespace's partial header build and repeats it.

The marker fingerprints the memory journal, so a namespace changed by a legacy
memory writer is rebuilt on the next open. Matching namespaces are skipped.
Initial rebuild work is proportional to retained current records and assertions;
it is not a constant-time migration. Rehearse an upgrade on a restored copy and
retain an appropriate backup before deploying against a large corpus.

Do not write to an upgraded volume using an intermediate experimental typed-
relation binary that predates these headers. Such a binary can change assertions
without advancing the memory journal; encountered inconsistencies then fail closed
instead of being silently repaired. Individual reads do not rebuild corrupt
indexes. Recovery must preserve subsequent acknowledged writes and immutable audit
history rather than overwriting a live database with an old snapshot.

## Use company administration

Obtain `workspace_id` from the
[company workspace directory](../security/company-memory-administration.md), then
send the same graph query with the administrative envelope:

```json
{
  "contract_version": 1,
  "workspace_id": "0000000000000000000000000000000000000000000000000000000000000000",
  "query": {
    "root_record_ids": ["018f0000-0000-4000-8000-000000000001"],
    "direction": "both"
  }
}
```

Replace the placeholder workspace ID with the authorized directory result.
Company graph reads still return only eligible memories and assertions. To inspect
retained metadata, send this body to the company `/relations/inspect` route:

```json
{
  "contract_version": 1,
  "workspace_id": "0000000000000000000000000000000000000000000000000000000000000000",
  "relation_id": "018f0000-0000-4000-8000-000000000003"
}
```

The response contains `company_id`, `workspace` and `inspection`. Add a positive
`revision` to the same body and use `/relations/revision` for immutable `history`.
Retained inspection can disclose a rejection or stale endpoint to the authorized
administrator; historical reads preserve the original relation and receipt even
after retirement. Neither route returns endpoint payloads or changes credentials.

## Freshness and error handling

All responses are `Cache-Control: no-store`. Missing/expired/revoked credentials
return 401, missing authority returns 403, malformed fields or bounds return 400,
and bodies over 65,536 bytes return 413. Native administrative reads return 404
`record_not_found` for unavailable workspaces, assertions or historical versions.
Scoped graph roots use root status within a 200 response instead of 404.
Occupied retrieval slots return 503 `retrieval_busy`; use bounded backoff.

The graph is a point-in-time observation, not a lease. Revalidate before reuse.
The separate [relation change feed](typed-relation-changes.md) now delivers
assertion lifecycle changes. Observe both memory and relation changes when
invalidating graph caches; a memory checkpoint alone does not certify a cached
graph as current. Expiration can still occur without an event. Feed cursors do
not paginate graph results or establish one atomic snapshot across both streams.
Automatic consolidation and graph-assisted ranking remain separate work.

Qualification uses controlled storage and real HTTP fixtures for direction,
cycles, repeated claims, exact revisions, scope/role isolation, native company
inspection, cuts, independent work counters, expiry, transitive invalidation,
corruption, bounded migration, incomplete-build recovery and abrupt process
termination. It does not establish production capacity or quality gains. See the
[research and evaluation map](../research/graph-memory-evidence.md) for the separate
retrieval and agent-task comparisons required before promoting a graph policy.
