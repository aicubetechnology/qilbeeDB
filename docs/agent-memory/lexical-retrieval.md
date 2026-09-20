# Deterministic lexical and hybrid retrieval

`AgentMemory::keyword_search` and `PersistentAgentMemory::keyword_search` rank
valid episodes with BM25. Existing `search_episodes` retains its substring
semantics. The new lexical API matches any complete query token, uses Unicode
alphanumeric tokenization and lowercase normalization, and searches primary,
secondary and context text. It does not stem words or perform Unicode canonical
normalization. Query tokens are deduplicated.

The implementation uses `k1 = 1.2`, `b = 0.75` and positive inverse document
frequency `ln(1 + (N - df + 0.5) / (df + 0.5))`. Scores are ranking signals, not
probabilities. It scans all valid episodes to compute corpus statistics; there
is no persistent inverted index or sublinear complexity claim. BM25 background:
[Robertson and Zaragoza](https://www.staff.city.ac.uk/~sbrp622/papers/foundations_bm25_review.pdf).

Persistent hybrid search now fuses ranked lexical and semantic candidates with
weighted reciprocal rank fusion, using rank constant 60. Both lexical scores
and fused scores resolve ties by episode UUID. The semantic candidate generator
remains approximate HNSW; deterministic fusion does not make approximate
candidate generation deterministic. RRF reference:
[Cormack, Clarke and Buettcher](https://research.google/pubs/reciprocal-rank-fusion-outperforms-condorcet-and-individual-rank-learning-methods/).

Semantic weights must be finite and in `[0, 1]`; invalid values return a
validation error instead of being silently clamped. A zero-weight channel is
excluded from both retrieval and fusion. Without semantic support, lexical
weight becomes one. A zero result limit or blank hybrid query returns no
results without calling the embedding provider. Candidate count multiplication
uses saturating arithmetic.

The existing `HybridSearchResult.keyword_score` and `semantic_score` fields
contain rank contributions, not raw BM25 or embedding similarity. Use
`KeywordSearchResult.score` for raw BM25 scores. Lexical fallback now uses the
same RRF scale as hybrid results; callers should not depend on the former
fallback score scale.

Seven offline tests cover ranking, corpus-order invariance, query token
deduplication, Unicode and context matching, invalidation, the BM25 formula,
stable ties, invalid weights, provider bypass and lexical fallback. These tests
establish behavior; external retrieval quality remains unmeasured.

## Durable platform records

`RocksDbMemoryStorage::search_memory_lexical(namespace, &LexicalQuery)` applies
the same tokenizer and BM25 formula to revisioned platform records. It requires
no vectors and searches existing records without rebuilding or migrating data.
The caller must authenticate and derive the namespace before calling this
low-level Rust method.

The query supplies `text`, `limit` (1–100), optional `after`, `episode_type` and
`tag`, `scan_limit` (1–10000, default 10000), and `scan_bytes_limit` (1–67108864,
default 8388608). Text is limited to 4096 UTF-8 bytes and 64 distinct lowercase
alphanumeric terms; empty/punctuation-only text is rejected. All record and
integrity reads use one request-local RocksDB snapshot and visibility timestamp.
Expired, deleted and filtered records contribute neither hits nor statistics.
Only primary, secondary and context text contribute terms; metadata is not searched.

The page contains `hits` (record plus raw BM25 `score`), `matched_records` before
result truncation, `corpus_records` contributing to statistics, `scanned_records`
including invisible/filtered rows, `scanned_bytes` of serialized source records,
`next_after` and `exhaustive`. Scores are not probabilities. Ties use ascending UUID.

A bounded scan is a UUID-ordered corpus page, not a search index. An unvisited
record produces `next_after` and `exhaustive: false`; a continued request always
reports false, even on its final page. A byte budget that cannot fit the first
record returns a validation error instead of a cursor that makes no progress.
Budgeted bytes exclude RocksDB indexes, keys, allocation overhead and the one
lookahead record used to detect a remainder; this is not a process memory cap.

**BM25 statistics are local to the filtered scanned corpus.** Scores from separate
pages cannot be merged into a global BM25 ranking. Use a sufficiently large
budget or a narrower scope/filter and require `exhaustive: true` for a complete
ranking. A snapshot only lasts for its request. This implementation does not
claim an inverted index or sublinear retrieval time.
