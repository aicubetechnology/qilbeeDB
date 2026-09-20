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
both weights at 0.5 and the rank constant at 60. Weighted reciprocal rank fusion
then assigns:

```text
lexical contribution = (1 - semantic_weight) / (60 + lexical_rank)
semantic contribution = semantic_weight / (60 + semantic_rank)
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

## Configure candidate and scan budgets

| Query field | Contract |
| --- | --- |
| `text` | Required, at most 4096 UTF-8 bytes, 1–64 distinct lowercase alphanumeric terms |
| `space`, `vector` | Required external model identity and finite, nonzero float32 query vector; same validation as semantic search |
| `limit` | Final result count, 1–100 |
| `ranking_version` | Required `weighted_rrf_v1`; selects an immutable server-defined method and parameters |
| `min_score` | Minimum raw cosine, finite in [-1, 1], default -1; does not filter lexical matches |
| `scan_limit` | At most 1–10000 source records, default 10000 |
| `scan_bytes_limit` | 1–67108864 serialized bytes, default 8388608 |
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
| `rank_constant` | The implemented RRF constant, 60 |
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
Choose weights on development data and report held-out relevance and latency.

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
limit is 65536 bytes, including the text, vector, scope and formatting.
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
