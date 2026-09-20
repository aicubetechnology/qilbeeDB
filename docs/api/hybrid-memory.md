# Hybrid memory retrieval

Hybrid retrieval combines exact lexical terms with similarity from externally
generated embeddings. Use it when a query contains names, identifiers or technical
terms but relevant memories may also express the same idea in different words.
QilbeeDB retrieves evidence; your application decides how to use it.

Use `POST /api/v1/memory/search/hybrid` with `memory_read` and an exact scope
grant. The existing `POST /api/v1/memory/search` continues to return cosine scores;
it is not reinterpreted as hybrid search. This new method is **experimental**:
functional validation does not establish a held-out relevance gain.

```json
{
  "contract_version": 1,
  "mode": "hybrid",
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "query": {
    "text": "ZX17 retry failure",
    "space": {"provider":"fixture","model":"fixture-embedding","revision":"v1","dimensions":3},
    "vector": [1.0, 0.0, 0.0],
    "ranking_version": "weighted_rrf_v1",
    "limit": 10,
    "min_score": 0.0,
    "scan_limit": 10000,
    "scan_bytes_limit": 8388608
  }
}
```

These three-dimensional vectors illustrate the contract, not language-model
relevance. Generate real document and query vectors externally. No provider
credential is accepted or required by QilbeeDB.

The response envelope contains `contract_version`, `scope`, `mode: "hybrid"`,
`ranking_version` and `page`. Rust applications can use
`RocksDbMemoryStorage::search_memory_hybrid(namespace, &HybridQuery)` after
performing their own authentication and namespace derivation.

## Keep model generation outside the database

Generate document and query vectors with the same external embedding service and
immutable model configuration. Attach document vectors using the
[embedding contract](semantic-memory.md). QilbeeDB does not choose a provider,
load an embedding model, rewrite text with a language model, or call a reranker.
Provider identity is a client declaration, not a provider attestation.

A source update invalidates its previous embedding. Until its new vector is
attached, the current memory can still match by BM25. A wrong or missing model
space yields no semantic candidates; lexical candidates remain available. The
response exposes this through `embedded_records` and per-hit contributions.

## Understand ranking

Both channels read the same authorized record corpus, RocksDB snapshot and
visibility timestamp. Deleted, expired and filtered memories are excluded before
corpus statistics and ranking. BM25 uses primary, secondary and context text,
Unicode alphanumeric tokens lowercased without stemming, `k1 = 1.2`, and `b = 0.75`.
It does not search metadata or perform substring matching.

Each channel sorts its matches by descending raw score, then ascending UUID, and
keeps up to 100 candidates. The server-owned `weighted_rrf_v1` profile fixes
both weights at 0.5 and the rank constant at 60. The separately versioned
`weighted_rrf_v2` uses lexical weight 0.25, semantic weight 0.75 and rank constant 2.
Both remain experimental and use the same 100-candidate cap. A request must name
its version explicitly; publishing v2 does not change v1 or select a new default.
Weighted reciprocal rank fusion assigns:

```text
lexical contribution = lexical_weight / (rank_constant + lexical_rank)
semantic contribution = semantic_weight / (rank_constant + semantic_rank)
fused score = lexical contribution + semantic contribution
```

Ranks start at one. An absent candidate contributes zero. Weights are **not**
renormalized when one channel has no matches. The candidate union is deduplicated by record UUID, sorted by fused
score then UUID, and truncated to `limit`. BM25, cosine and fused scores are
ranking signals, not probabilities or calibrated confidence.

`hits[].lexical` and `hits[].semantic` each contain `rank`, raw `score`, and
weighted `contribution`, or null if absent from that candidate list.
`hits[].embedding` is the original revision-bound receipt when the semantic
channel contributes, otherwise null. The record always includes its source
revision, author, validity and payload. A source can change after the snapshot;
use the returned revision for subsequent conditional writes.

## Discover the server's supported profiles

Call `GET /api/v1/memory/ranking-profiles` with a current `memory_read` credential.
No scope or query body is required because this endpoint reads only server-wide
implementation metadata. It returns no memory records, corpus statistics,
registered embedding spaces or tenant configuration.

```json
{
  "contract_version": 1,
  "component_versions": {"lexical": "bm25_v1", "semantic": "cosine_exact_v1"},
  "execution_limits": {
    "scope": "server_instance",
    "max_embedding_dimensions": 32768,
    "max_scan_bytes": 67108864,
    "default_scan_bytes": 8388608,
    "max_concurrent_retrievals": 2,
    "vector_request_body_bytes": 2097152
  },
  "hybrid_profiles": [{
    "version": "weighted_rrf_v1", "method": "weighted_rrf",
    "lexical_version": "bm25_v1", "semantic_version": "cosine_exact_v1",
    "candidate_limit": 100, "lexical_weight": 0.5, "semantic_weight": 0.5,
    "rank_constant": 60, "experimental": true
  }, {
    "version": "weighted_rrf_v2", "method": "weighted_rrf",
    "lexical_version": "bm25_v1", "semantic_version": "cosine_exact_v1",
    "candidate_limit": 100, "lexical_weight": 0.25, "semantic_weight": 0.75,
    "rank_constant": 2, "experimental": true
  }]
}
```

Discovery and retrieval use the same profile definitions. Compare a selected
profile with the version and `page.ranking` returned during execution; fail closed
if an evaluation's pinned profile differs. Missing, expired or revoked credentials
return 401; a credential without `memory_read` returns 403, including an operator
credential that grants only credential/policy administration. Responses use
`Cache-Control: no-store`; do not use a cached catalog as authorization evidence.
Each actual search still requires its own exact scope grant.

A listed method is supported by the running implementation. Listing does not admit
it under enterprise policy, qualify its relevance or select it as a default.
`weighted_rrf_v1` remains unchanged and experimental. The available development
results do not currently justify publishing another set of weights.

## Configure candidate and scan budgets

| Query field | Contract |
| --- | --- |
| `text` | Required, at most 4096 UTF-8 bytes, 1–64 distinct lowercase alphanumeric terms |
| `space`, `vector` | Required external model identity and finite, nonzero float32 query vector; same validation as semantic search |
| `limit` | Final result count, 1–100 |
| `ranking_version` | Required `weighted_rrf_v1`; selects an immutable server-defined method and parameters |
| `min_score` | Minimum raw cosine, finite in [-1, 1], default -1; does not filter lexical matches |
| `scan_limit` | At most 1–10000 source records, default 10000 |
| `scan_bytes_limit` | 1–268435456 serialized bytes, subject to the operator ceiling (default 67108864); request default 8388608 |
| `after` | Optional source UUID cursor; a continuation starts a new snapshot |
| `episode_type`, `tag` | Optional exact filters applied to both channels before scoring |

The hybrid contract always validates text and vector. Use
[lexical retrieval](lexical-memory.md) when no query embedding is available.
Clients cannot override weights, the method or candidate cap in the request;
unknown fields are rejected. The QilbeeDB team publishes a new ranking version
when these parameters or algorithm semantics change. `page.ranking` returns the
exact method, weights, candidate cap, rank constant, component versions
(`bm25_v1`, `cosine_exact_v1`) and experimental status.

The scan and body ceilings are implementation safeguards, not a tenant quota or
an enterprise admission policy. Experimental evaluation budgets must be reported
separately from an organization's production limits. This release does not add
tenant-specific retrieval quotas or concurrency admission.

The byte budget counts serialized source records and selected-space bindings read
for visible, filtered records, including stale bindings. It excludes keys,
integrity indexes, allocation overhead and one lookahead row; it is not a process
memory cap. A budget too small for the first record and binding returns a
validation error. No cursor is returned that silently skips an oversized row.

## Interpret coverage before using results

| Page field | Meaning |
| --- | --- |
| `scanned_records`, `scanned_bytes` | Work admitted to this source corpus page |
| `corpus_records` | Visible records passing filters, including those without embeddings |
| `embedded_records` | Corpus records with a current binding in the selected space |
| `lexical_matches`, `semantic_matches` | Matches in each enabled channel before candidate truncation |
| `lexical_candidates`, `semantic_candidates` | Candidates retained by each channel |
| `candidates_truncated` | At least one channel dropped matches at its candidate limit |
| `rank_constant` | The selected immutable RRF constant: 60 for v1, 2 for v2 |
| `ranking` | Exact immutable server profile and experimental status |
| `embedding_coverage` | `complete`, `partial`, `missing`, or `empty_corpus`, relative to the visible filtered corpus in this scan |
| `next_after` | Last scanned source UUID if another row remains; otherwise null |
| `exhaustive` | This request began without a cursor and scanned the whole scope |

`exhaustive: true` describes corpus coverage, **not** unlimited candidate lists.
Check `candidates_truncated` separately. Missing embeddings do not make the source
scan partial; compare `embedded_records` with `corpus_records` to inspect coverage.
`embedding_coverage: missing` means every returned hit is lexical-only;
`partial` means some eligible memories lack a current selected-space vector.
Neither state claims complete hybrid coverage. An empty selected model space is
reported explicitly this way. Neither field certifies vector origin.

A storage read, integrity or ranking validation failure fails the **entire**
request. The implementation does not catch a failed channel and silently return
another channel's ranking. Missing bindings are data coverage, not a swallowed
execution failure. Lexical and dense channels are always attempted in this profile.

When `exhaustive` is false, rankings and BM25 statistics describe only the scanned
page. Do not merge page scores as if they were a global ranking. Narrow the scope
or filters, or raise the scan budget within its limits. Continuations always
report `exhaustive: false`, including the final page. They do not preserve a
snapshot across requests. This implementation is a bounded full scan, not ANN
or an inverted index.

## Choose settings with evidence

The defaults are a starting point, not an assertion that hybrid search improves
every workload. Evaluate BM25, dense and hybrid retrieval on the same judged
queries, frozen source corpus and externally generated vectors. Keep candidate
budgets and output `k` explicit; report missing vectors and incomplete scans.
The QilbeeDB team selects new server-owned profiles on development data and
reports held-out relevance and latency; callers select the immutable version.

The design uses [reciprocal rank fusion (Cormack, Clarke and Buettcher, SIGIR
2009)](https://research.google/pubs/reciprocal-rank-fusion-outperforms-condorcet-and-individual-rank-learning-methods/).
Research on [contextual retrieval](https://www.anthropic.com/engineering/contextual-retrieval)
also motivates combining lexical and dense evidence. Those published results do
not establish QilbeeDB's relevance, and this implementation does not perform
context generation or reranking. Synthetic vector tests verify contracts, not
language understanding or downstream agent improvement.

## Errors and compatibility

Requests require `mode: "hybrid"`; a mode that does not match the endpoint returns
400. Invalid vectors, unknown ranking versions, caller-supplied weights or
candidate caps, unsupported contract versions and invalid budgets also return
400. Missing credentials return 401; revoked or expired credentials return 401;
missing capability or scope returns 403; oversized bodies return 413; encountered
storage corruption or unsupported stored versions return 500. The JSON body
limit is 2097152 bytes (2 MiB), including text, vector, scope and formatting.
Dimensions range from 1 through 32768, subject to the operator ceiling; 3072 is
supported without changing cosine or fusion semantics.
See the [platform error envelope](platform-http.md).

The tenant is derived from the authenticated credential. Project, agent, mission
and visibility must match an exact grant. Private namespaces also include the
current subject. Both channels and their corpus statistics use that namespace
before candidate selection. No cross-scope ranking cache is used. Revoked
credentials cannot begin new authorized searches; an already-authorized request
can complete against its snapshot. A subsequent request observes source updates,
deletions and expiry. Expiry is fixed at each request's visibility timestamp.

The UUID cursor traverses source records, not ranked hits or a durable snapshot.
Its exclusive lower bound prevents returning the same source UUID again on a
forward continuation, but inserts or updates across requests can change coverage.
It cannot reproduce a multi-page global ranking under concurrent mutation.

## Measure retrieval time

The response includes `timing.retrieval_micros`, the server wall time spent in
the retrieval method. It excludes authentication, blocking-pool queueing, JSON
serialization, transport and external embedding generation. Measure the client
round trip separately. See the [reproducible evaluation workflow](../research/retrieval-evaluation.md)
for frozen corpora, graded relevance, category regressions and timing limits.

## Execution capacity

The ranking catalog exposes the current server dimension, scan-byte and concurrent
retrieval limits. These are operator settings, separate from the immutable ranking
profile and from tenant authorization. A byte budget above the configured ceiling
returns 400 (`retrieval_scan_limit`); exhausted retrieval slots return 503
(`retrieval_busy`). Use bounded backoff and inspect coverage on each successful
response. See [configure retrieval capacity](../operations/retrieval-capacity.md).

See the [0.6.0 real-embedding report](../research/scifact-results.md) for the
300-query SciFact comparison, uncertainty and losses. Both profiles remain
experimental; the measured gain does not establish a universal ranking policy.

## Source-dependent eligibility

[Derived memories](derived-memory.md) validate their declared sources in the
request snapshot before candidate eligibility and corpus statistics. Pages report
additional source reads in `dependency_work`; these are separate from candidate
scan budgets. Exceeding the documented dependency limits fails the request.

## Score bounds and response validation

Weighted RRF combines reciprocal **ranks**, not raw BM25 and cosine values. With
one-based ranks, each channel's maximum contribution is its weight divided by
`rank_constant + 1`. The maximum combined score is the sum of those contributions
when the same record ranks first in both channels.

| Ranking version | Constant | Lexical maximum | Semantic maximum | Combined maximum |
| --- | ---: | ---: | ---: | ---: |
| `weighted_rrf_v1` | 60 | 0.5 / 61 = 0.00819672131147541 | 0.5 / 61 = 0.00819672131147541 | 1 / 61 = 0.01639344262295082 |
| `weighted_rrf_v2` | 2 | 0.25 / 3 = 0.08333333333333333 | 0.75 / 3 = 0.25 | 1 / 3 = 0.3333333333333333 |

A v2 semantic-only result at score 0.25 is legitimate. A missing channel contributes
zero; scores remain weighted RRF values, neither cosine nor calibrated probability.
The HTTP endpoint and `contract_version: 1` are unchanged: `weighted_rrf_v2` names
the ranking profile, not an API version.

Starting in **0.9.0**, the published OpenAPI contract applies `HybridHitV1` or
`HybridHitV2` through `HybridPage.ranking.version`, including per-channel contribution
limits. `HybridResponse` also enforces agreement between the envelope's
`ranking_version`, the page profile and its constant. The generic `HybridHit` and
`RankContribution` definitions cover all accepted profiles; clients validating
whole responses should use the endpoint's response schema to retain the more
specific version checks.

Earlier contracts in 0.7.0 and 0.8.0 incorrectly applied v1's maximum combined
score and channel contribution to v2. Valid v2 HTTP 200 responses could therefore
fail client-side schema validation. This correction changes documentation and
validation, not ranking weights, computed scores, ordering or the cosine endpoint.
Fetch `/openapi.json` from the deployed server and refresh cached/generated client
contracts when upgrading.

The regression suite starts a real loopback HTTP server, downloads its published
OpenAPI, and validates v1 and v2 responses for both-channel, lexical-only,
semantic-only and empty results. It also rejects scores or contributions above
the selected profile's bounds and mismatched ranking identities. These synthetic
HTTP fixtures establish schema conformance only. Neither they nor a query made
during corpus import establish retrieval relevance or comparative ranking quality.
