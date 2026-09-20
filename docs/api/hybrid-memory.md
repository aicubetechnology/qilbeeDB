# Hybrid memory retrieval

Hybrid retrieval combines exact lexical terms with similarity from externally
generated embeddings. Use it when a query contains names, identifiers or technical
terms but relevant memories may also express the same idea in different words.
QilbeeDB retrieves evidence; your application decides how to use it.

The Rust entry point is `RocksDbMemoryStorage::search_memory_hybrid(namespace,
&HybridQuery)`. Authenticate and derive the namespace before calling this
low-level method. HTTP transport is added separately in the next feature.

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
keeps up to `candidate_limit`. Weighted reciprocal rank fusion then assigns:

```text
lexical contribution = (1 - semantic_weight) / (60 + lexical_rank)
semantic contribution = semantic_weight / (60 + semantic_rank)
fused score = lexical contribution + semantic contribution
```

Ranks start at one. An absent candidate contributes zero. A zero-weight channel
is excluded, and remaining weights are **not** renormalized when one channel has
no matches. The candidate union is deduplicated by record UUID, sorted by fused
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
| `candidate_limit` | Per-channel candidate count, at least `limit` and at most 1000; default 100 |
| `semantic_weight` | Finite value in [0, 1], default 0.5; lexical weight is its complement |
| `min_score` | Minimum raw cosine, finite in [-1, 1], default -1; does not filter lexical matches |
| `scan_limit` | At most 1–10000 source records, default 10000 |
| `scan_bytes_limit` | 1–67108864 serialized bytes, default 8388608 |
| `after` | Optional source UUID cursor; a continuation starts a new snapshot |
| `episode_type`, `tag` | Optional exact filters applied to both channels before scoring |

The hybrid contract always validates text and vector, including zero-weight
configurations. Use lexical retrieval when no query embedding is available.

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
| `embedded_records` | Corpus records with a current binding in the selected space; zero when semantic weight is zero |
| `lexical_matches`, `semantic_matches` | Matches in each enabled channel before candidate truncation |
| `lexical_candidates`, `semantic_candidates` | Candidates retained by each channel |
| `candidates_truncated` | At least one channel dropped matches at its candidate limit |
| `rank_constant` | The implemented RRF constant, 60 |
| `next_after` | Last scanned source UUID if another row remains; otherwise null |
| `exhaustive` | This request began without a cursor and scanned the whole scope |

`exhaustive: true` describes corpus coverage, **not** unlimited candidate lists.
Check `candidates_truncated` separately. Missing embeddings do not make the source
scan partial; compare `embedded_records` with `corpus_records` to inspect coverage.
Neither field certifies that an embedding provider produced the vectors.

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
