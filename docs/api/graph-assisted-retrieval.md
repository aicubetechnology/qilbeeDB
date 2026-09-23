# Experimental graph-assisted retrieval

Use graph-assisted retrieval when relevant memories may be connected by explicit
entity, temporal, semantic or evidential assertions that a direct query misses.
This **0.13.0** API selects initial memories, traverses their typed
relations and returns ranked memories with the exact paths used. It is opt-in and
experimental. No relevance, latency or agent-task improvement has been established.

The existing [cosine](semantic-memory.md), [BM25](lexical-memory.md) and
[hybrid](hybrid-memory.md) endpoints keep their score meanings and defaults.
Graph search does not create assertions, infer entities, call a model or generate
embeddings. Applications supply [typed relations](typed-memory-relations.md) with
provenance and, for vector queries, externally generated vectors with complete
provider, model, revision and dimension identities.

## Request a graph-assisted result

`POST /api/v1/memory/search/graph` requires `memory_read` in the exact requested
scope. Company and private subject come from the authenticated credential.
Project, agent, mission and visibility are authorized before candidate selection,
statistics, adjacency lookup or shared retrieval admission. Responses use
`Cache-Control: no-store`.

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "project",
    "agent_id": "agent",
    "mission_id": null,
    "visibility": "private"
  },
  "query": {
    "ranking_version": "typed_path_evidence_v1",
    "seed": {"mode": "lexical", "text": "cache invalidation"},
    "limit": 10
  }
}
```

The response contains `contract_version`, `scope` and `page`. The page includes
the immutable ranking profile, snapshot evaluation time, seed selection metadata,
graph and embedding work, coverage, final hits and the relations used by those
hits' paths. Empty seed results produce an empty graph; the server does not
substitute unrelated roots.

Choose a seed mode explicitly:

| Mode | Required seed fields | Native score retained in `hit.base.retrieval` |
| --- | --- | --- |
| `lexical` | `text` | BM25 from `bm25_v1` |
| `semantic` | `space`, `vector` | Cosine from `cosine_exact_v1` |
| `hybrid` | `text`, `space`, `vector`, `ranking_version` | Weighted RRF with separate lexical and cosine contributions |

For hybrid seeds, the inner `seed.ranking_version` is `weighted_rrf_v1` or
`weighted_rrf_v2`. The outer `query.ranking_version` selects the graph method.
Vector modes accept `seed.min_score` in [-1, 1], default -1. This threshold selects
seeds only: a connected memory can be returned below it or without an embedding.
Missing or stale bindings are explicitly reported. Vectors from different spaces
are never compared, and an unidentified historical vector is not assigned a model.

For example, replace `query.seed` with:

```json
{
  "mode": "hybrid",
  "text": "cache invalidation",
  "space": {
    "provider": "fixture",
    "model": "example-model",
    "revision": "v1",
    "dimensions": 3
  },
  "vector": [1.0, 0.0, 0.0],
  "ranking_version": "weighted_rrf_v2",
  "min_score": -1.0
}
```

This three-dimensional vector demonstrates the request shape only. Use vectors
from your declared external model in a real application; production dimensions
are bounded by the [server execution policy](../operations/retrieval-capacity.md).

## Immutable profiles and scoring

`GET /api/v1/memory/graph-ranking-profiles` requires `memory_read` and returns
`graph_profiles` plus `execution_limits`. Pin the selected version. Clients cannot
send weights, a different anchor count or an unversioned `latest` method.

All four initial profiles retain at most 100 base candidates and choose the first
four as graph anchors. Base ties use ascending record UUID. The initial relation
weights are:

| Profile | Semantic | Same entity | Temporal before | Causal claim | Supports | Contradicts |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `typed_path_balanced_v1` | 1 | 1 | 1 | 1 | 1 | 1 |
| `typed_path_entity_v1` | 0.5 | 1 | 0 | 0 | 0 | 0 |
| `typed_path_temporal_v1` | 0.25 | 0.5 | 1 | 0 | 0 | 0 |
| `typed_path_evidence_v1` | 0.25 | 0.5 | 0.25 | 0.75 | 1 | 1 |

Zero-weight kinds are excluded before expansion. These values are experimental
hypotheses, not learned or benchmark-selected parameters. A contradiction is
retrievable counterevidence, not a negative truth score. A causal assertion remains
an attributed claim. Repeated claims are not independent corroboration.

An anchor at one-based base rank `r` starts with strength `1 / (2 + r)`. Each
traversed edge multiplies strength by `0.5 * relation_weight * destination_affinity`.
For lexical queries affinity is 1. For vector queries a current selected-space
binding gives `0.5 + 0.5 * max(cosine, 0)`; a missing or stale binding gives 0.5 and
appears in the path's `missing_affinity`. Zero-hop anchors retain their initial
strength. Work accounting distinguishes reused base cosines from fresh lookups.

For each reached memory, keep the **maximum** path strength, not a sum. Ties prefer
fewer hops, lower anchor rank and then lexicographic relation UUID/direction steps.
Graph ranks order strength descending, hops ascending, anchor rank ascending and
record UUID ascending. Relation UUIDs choose a proof but do not break graph rank
ties. Within an uncut neighborhood, adding equal parallel assertions cannot inflate
rank, and cycling cannot increase strength. Added edges can still consume bounded
traversal work; compare coverage as well as scores.

The final score is:

```text
score = (base present ? 0.25 / (2 + base_rank) : 0)
      + (graph present ? 0.75 / (2 + graph_rank) : 0)
```

Final ties use ascending record UUID. The maximum for every current graph version
is `0.25/3 + 0.75/3 = 1/3`. This is neither cosine nor a probability. The published
OpenAPI fixes each profile's parameters and validates real HTTP responses for all
four versions. Raw native scores and external vector receipts remain separate.

## Paths and current memory authority

Each hit contains its current canonical `record`, combined `score`, optional
`base`, optional `graph` and optional current `affinity`. A graph contribution
identifies the anchor revision, anchor rank, graph rank, path strength and exact
relation revisions with `from`, `to` and traversal direction for every step.
`page.relations` contains only the complete assertions used by returned paths,
including provenance, state and review. Incoming traversal does not rewrite a
relation's original direction or turn a temporal/causal claim into its converse.

The requested `limit` bounds canonical record payloads returned. Intermediate
memories are referenced by ID and revision in the proof; their payloads are not
silently added beyond that context budget. A later read is a new observation.
Revalidate current records and assertions before reuse where freshness matters.

Seed selection, typed traversal, affinity reads and ranking share one RocksDB
snapshot, eligibility cache and clock. An eligible edge requires current endpoint
revisions and eligible canonical memories. Deleted, expired or rejected endpoints,
stale assertions and invalid derived-memory sources cannot serve as bridges.
`episode_type` and `tag`, when supplied at query level, apply to both seed selection
and neighbors **before inclusion or expansion**. Examining an excluded neighbor
can still consume bounded work. Queries do not cross the authorized namespace.

The 0.14.0 [relation context extension](typed-memory-relations.md#bind-the-context-used-for-inference)
requires every declared additional source and its ancestry to remain current and
eligible before an edge can contribute a path. Invalid context removes that path,
not an independently eligible lexical or vector candidate. This does not change
profile weights or score meanings. Extra context validation is reported in the
cumulative dependency work; its cost must be included in future comparisons.

## Limits, coverage and failure handling

| Field | Default | Allowed values |
| --- | ---: | --- |
| `limit` | Required | 1–100 returned payloads |
| `scan_limit` | 10,000 | 1–10,000 base current candidates |
| `scan_bytes_limit` | 8 MiB | 1–256 MiB, subject to the server ceiling |
| `expansion.direction` | `both` | `outgoing`, `incoming`, `both` |
| `expansion.max_depth` | 2 | 0–8 hops |
| `expansion.node_limit` | 128 | 1–256 canonical lookups, including unavailable/filtered memories |
| `expansion.edge_limit` | 256 | 1–1,024 eligible assertions |
| `expansion.scan_limit` | 1,024 | 1–4,096 physical adjacency entries |
| `expansion.embedding_bytes_limit` | 8 MiB | 1–8 MiB of newly read selected-space bindings |

Omit `expansion` for these defaults. If supplied, include every expansion field.
The request body is limited to 2 MiB (2,097,152 bytes), including all JSON fields
and whitespace. The OpenAPI operation records this in
`x-qilbee-max-request-body-bytes` and its HTTP 413 description. Lexical text is at most 4,096 UTF-8 bytes
with 1–64 distinct alphanumeric terms, using the existing BM25 tokenizer.

Base scanning follows the current-record index, so deletion tombstones do not
consume all candidate slots. Base candidate and byte cuts are distinct from the
100-candidate ranking cap. A hybrid base also has the existing 100-candidate cap
per channel; inspect both `base_candidates_truncated` and
`channel_candidates_truncated`. BM25 statistics belong to the scanned authorized
corpus, so scores from separate cut scans are not globally comparable.

Graph expansion uses the [typed traversal order and bounds](typed-memory-graph.md):
breadth first, roots in base-rank order, outgoing before incoming, and ascending
relation UUID within each endpoint/revision index. Physical work includes repeated,
filtered and stale adjacency. Canonical bytes are capped at 8 MiB, relation bytes
at 4 MiB; shared dependency work at 4,096 records/16 MiB with existing per-record
ancestry limits. Path scoring examines the returned bounded graph for at most
eight depths and two directions per edge (at most 16,384 edge relaxations).

`seed.dependency_work` is the count after base selection.
`graph_coverage.dependency_work` is cumulative for the **whole request**. Do not
add them. Seed/index/graph/binding byte counters are logical accounted bytes,
not physical I/O, process RSS or total response size; decoding, integrity indexes,
lookahead and serialization are additional work. A single lookahead record/binding
can be read to discover byte exhaustion.

| Coverage flag | Meaning |
| --- | --- |
| `source_complete` | Base candidate scan exhausted its authorized filtered source |
| `candidates_complete` | Neither the base pool nor a hybrid channel was truncated |
| `graph_complete` | No depth, node, edge or adjacency-scan cut for the selected anchors/kinds |
| `embeddings_complete` | Requested-space coverage is complete within admitted base/graph records, or the query is lexical |
| `complete` | All four conditions above hold |

Completeness is relative to this fixed four-anchor profile. It is not a claim that
the query explored every component of the company's graph or found all relevant
memories. Missing vectors, even for unreturned inspected graph nodes, conservatively
make embedding coverage incomplete. No storage error becomes a successful lexical
fallback. Empty eligible corpus is separately identified in seed embedding coverage.

There is **no continuation cursor** for graph ranking. A larger-budget retry reads
a new snapshot and may produce a different ordering. Do not concatenate pages or
interpret a cut result as a global top-k. Check `coverage` and
`graph_coverage.stop_reasons` before caching or comparing results.

HTTP 400 reports invalid input or hard canonical/relation/dependency/affinity byte
exhaustion; configured vector/scan ceilings use `embedding_dimension_limit` and
`retrieval_scan_limit`. HTTP 401/403 indicate credential or authorization failures,
413 an oversized body and 503 `retrieval_busy` shared admission exhaustion. Corrupt
storage fails the request. Hard failures return no successful partial page. Retry
503 with backoff; revise budgets/input for 400 rather than silently changing mode.

## Evidence and research boundary

[MAGMA](https://arxiv.org/html/2601.03236v2) studies query-adaptive retrieval across
semantic, entity, temporal and causal memory graphs. It motivates explicit typed
policies and path inspection here. QilbeeDB's bounded strongest-path RRF is an
independent design; it does not reproduce MAGMA's learned components or its results.
The [graph-based agent memory survey](https://arxiv.org/html/2602.05665v1) informs
the separation of storage, retrieval and evolution contracts, not a measured gain.
[ReasoningBank](https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/)
motivates evaluating experience reuse on agent tasks with success and failure
evidence; this endpoint does not implement that learning loop.

Functional tests establish scope, revision, snapshot, bounds and scoring behavior
with synthetic fixtures. Admission as a default requires a separate frozen corpus,
development/test split, graded relevance judgments, equal final context budgets,
identical external embeddings, category regressions, latency/resource costs and
reported uncertainty. Relation extraction cost and missing/incorrect assertions
must be measured. Agent capability requires a further controlled task comparison
with fixed model, prompts and tools. Neither test class substitutes for the other.

See [retrieval evaluation](../research/retrieval-evaluation.md) for the comparison
protocol and [OpenAPI](openapi.json) for exact wire shapes.

`X-Qilbee-Retrieval-Micros` reports monotonic elapsed time after authorization and
retrieval admission, covering seed selection, traversal, affinities and ranking.
It excludes JSON serialization, network transport and external embedding generation.
Measure complete client latency separately and preserve query-embedding timing
when estimating end-to-end costs; the header is elapsed wall time, not CPU time.

### Validate ranking evidence before comparing methods

**Availability: source preview.** These additional comparison-runner checks are
being qualified and do not change the deployed retrieval API.

The graph comparison runner rejects a response when its returned scores are
nonfinite, outside the method's range, or inconsistent with the documented
ordering. Scores descend; equal scores use ascending record UUIDs. The validator
preserves the server's floating-point ordering of positive and negative zero.
It does not sort a malformed response to make the experiment pass.

Cosine scores must be within [-1, 1], lexical scores must be nonnegative, and
hybrid scores must respect the selected immutable profile's maximum. Graph
scores also retain their profile and contribution checks. Numeric-looking
strings, booleans and nonfinite numbers are not valid score evidence.

These checks apply before the runner accepts ranked IDs for relevance metrics.
They establish response consistency, not that every lexical score or omitted
candidate has been independently recomputed. A valid ordering alone does not
prove retrieval quality or improved agent reasoning.

The source-preview comparison tools also verify each returned semantic baseline
score and hybrid semantic-channel score against the frozen external vectors,
after float32 conversion. A contributing semantic channel must carry its exact
frozen embedding receipt; a hybrid result without a semantic contribution must
not attach one. See [semantic evidence verification](semantic-memory.md#verify-semantic-evidence-in-retrieval-comparisons)
for tolerances and limits. This additional check does not establish global
optimality of the ranking or validate omitted candidates.

## Investigate changes on development queries

**Availability: source preview for comparison tooling.** Use the separate
`benchmarks/retrieval/graph-multihop-development-v1.json` protocol with
`scripts/evaluate_graph_retrieval.py` to investigate the existing methods on the
fixture's development split. The reserved comparison protocol remains unchanged.

The development protocol retains the same eight methods, external vectors,
server-owned ranking profiles, result count and work budgets. It changes the
selected query split and the evidence stage. Its protocol identity cannot be
combined with the test split. The runner rejects an empty selected split before
query execution. Use new plan, state and report paths in an isolated authorized
benchmark environment; preserve prior reports, including failures.

Development reports and exports carry `evaluation_stage: development` and
`default_admission: false`. An export rejects test-query rows or a missing or
contradictory development-stage label. Existing reserved-comparison reports remain
readable. Keep model/provider costs, graph construction, source verification and
retrieval measurements distinct.

Use development findings to form a candidate hypothesis. Freeze any new ranking
method and its evaluation protocol before measuring a fresh reserved cohort.
Previously published test queries may be used as regressions, but do not reuse
them as independent confirmation after inspecting their results. A development
gain is neither a production promotion nor evidence of better agent reasoning.

## Verify comparison aggregates before sharing evidence

**Availability: source preview for comparison tooling.** The graph report exporter
recomputes category summaries and paired comparisons from verified query rows.
Each row must retain its category from the frozen fixture. Mean differences,
bootstrap intervals, win/loss/tie counts and the primary-comparison designation
must agree with the declared protocol and measured rows.

A mismatch stops the export; it is not silently repaired. Preserve the original
report and investigate its producer or changes before producing a new export.
This check detects inconsistent published conclusions. It does not establish
that the dataset is representative, that judgments are exhaustive, or that a
development improvement generalizes to a reserved evaluation or agent task.

## Evaluate the base-preserving profile

**Availability: source preview; experimental and opt-in.** Select
`typed_path_base_preserving_v1` explicitly in a graph search request. It uses the
same traversal and relation weights as `typed_path_balanced_v1`, with a base
contribution of `0.75 / (2 + base_rank)` and a graph contribution of
`0.25 / (2 + graph_rank)`. An absent contribution is zero. The maximum combined
score is `1/3`; this is neither cosine nor a probability. Equal scores retain the
existing record-ID tie-break. Existing profiles and defaults remain unchanged.

The profile gives more weight to seed ordering. Its name does not guarantee that
a particular result remains in the final list, nor that retrieval quality improves.
The company or agent application chooses whether to request it; the database
enforces the versioned ranking, authorized scope, current-source checks and work
limits. Clients should validate both contributions against the selected version.

Use `benchmarks/retrieval/graph-base-preserving-development-v1.json` to compare
the candidate with the eight existing methods on the same development queries,
external vectors and budgets. This separate protocol cannot be relabeled as the
previous development or reserved protocol. It declares the new candidate versus
hybrid v2 as its primary comparison. Freeze a fresh reserved evaluation before
using it to assess generalization; no improvement or default admission follows
from adding this optional profile.

## Best-channel experimental selection

**Availability: source preview.** Select `typed_path_best_channel_v1` explicitly
to compare a policy that uses the better reciprocal rank from the base and graph
channels. Its catalog method is `strongest_typed_path_max`:

```text
base_value = base present ? 1 / (2 + base_rank) : 0
graph_value = graph present ? 1 / (2 + graph_rank) : 0
score = max(base_value, graph_value)
```

The channel weights are both 1; they are independent scale factors, not mixing
probabilities. The maximum final score is 1/3. `base.contribution` and
`graph.contribution` expose the channel values; **do not sum them** for this
method. Ties use ascending record UUID. Native lexical/cosine scores retain their
own meaning. Check the authenticated catalog and the response's ranking version
before interpreting any combined score. A server that does not advertise this
version does not support it.

This policy keeps the same four anchors, bounded traversal, relation weights,
path proofs, affinity rules and current-source eligibility as the balanced
profile. It can admit graph-only evidence to the top ten even when ten base
candidates exist. That capability does not guarantee useful evidence: lexical
and graph neighborhoods can disagree, and useful base results can be displaced.
Repeated assertions still use the strongest path rather than accumulated votes.

All five previous profiles preserve their formulas and parameters. This version
is optional and experimental; a development-only screening result is not
independent evidence of superiority. Compare relevance, regressions, work and
coverage on your workload, with a separately frozen evaluation before admission.
No agent reasoning or token-efficiency improvement follows from this formula.

### Reproduce a best-channel development comparison

Use `benchmarks/retrieval/graph-best-channel-development-v1.json` with
`scripts/evaluate_graph_retrieval.py` to compare ten methods, including the
existing base-preserving profile. The declared primary comparison is
`graph_hybrid_best_channel` versus `weighted_rrf_v2`; the graph seed remains
hybrid v2. The protocol selects only the fixture's development queries. It
rejects a reserved split, missing/duplicated methods, a substituted ranking
version or a different declared primary comparison.

Freeze the source revision, server profile catalog, fixture, model space,
relations and protocol before running. Use an authorized disposable scope,
externally generated vectors and the same budgets for all methods. The existing
prepare/evaluate workflow records current source evidence, query-level scores,
coverage and resource observations. Export the completed report only after
checking the full method/query/repetition matrix.

This is a development experiment, including when it reproduces an earlier
prototype. The protocol does not authorize applying its result to an already
observed reserved cohort or changing a production default. Independent evaluation
and downstream task outcomes require their own frozen contracts.
