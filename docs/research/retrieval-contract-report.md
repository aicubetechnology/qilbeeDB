# Synthetic retrieval contract report — 0.5.0

Hybrid was **not admitted** by this trial. The frozen synthetic test queries show
lower mean nDCG@10 than BM25 and a critical error-code regression. The result is
retained without retuning the server profile against the reserved test queries.

This is a `synthetic_contract` comparison with hand-authored vectors and one test
query per category. It validates the comparison workflow, not language relevance.
Hybrid remains **experimental**. See the [full JSON evidence](retrieval-contract-report.json)
and [evaluation methodology](retrieval-evaluation.md).

Validated source: `88fdf11d45c41222d98f80b475394e38c6d6009e`.
Image: `sha256:e3cd37176fbf486e00d30f9b7ca75e0818222f3bb5f22f649b6f87da254a7508` (Linux ARM64). The isolated Docker trial verified
identical source UUIDs, revisions and rankings before and after SIGKILL. Its
throwaway volume was removed after validation. Exact replay of that UUID mapping
requires the original database; a fresh run creates a new manifest and may
change ties. The permanent local deployment retains a separate manifest for
repeatable integration trials.

| Method | Test nDCG@10 | Test Recall@10 | No useful result, answerable | Retrieval p50 / p95 ms | HTTP p50 / p95 ms | Mean response bytes |
| --- | --- | --- | --- | --- | --- | --- |
| lexical | 0.9637 | 1.0000 | 0.0000 | 0.4320 / 0.5012 | 2.0283 / 4.8436 | 2945.0000 |
| semantic | 0.8185 | 0.8125 | 0.1250 | 0.5290 / 0.8022 | 2.0049 / 4.2278 | 13477.5556 |
| hybrid | 0.8717 | 0.8750 | 0.1250 | 0.8360 / 0.9471 | 2.4484 / 4.5980 | 15228.1481 |

## Per-category held-out results

| Category | Queries per method | Lexical nDCG@10 | Semantic nDCG@10 | Hybrid nDCG@10 |
| --- | --- | --- | --- | --- |
| ambiguous_terms | 1 | 1.0000 | 1.0000 | 1.0000 |
| error_codes | 1 | 1.0000 | 0.0000 | 0.0000 |
| exact_identifiers | 1 | 1.0000 | 0.6309 | 1.0000 |
| file_and_class_names | 1 | 1.0000 | 1.0000 | 1.0000 |
| near_duplicates | 1 | 1.0000 | 0.9173 | 0.9738 |
| noise | 1 | 1.0000 | 1.0000 | 1.0000 |
| paraphrases | 1 | 0.7098 | 1.0000 | 1.0000 |
| portuguese | 1 | 1.0000 | 1.0000 | 1.0000 |
| unanswerable | 1 | unavailable | unavailable | unavailable |

## Paired uncertainty and losses

- Hybrid minus lexical: -0.0920; exploratory 95% paired bootstrap interval [-0.3783, 0.1055], 8 answerable test queries.
- Hybrid minus semantic: 0.0532; exploratory 95% paired bootstrap interval [0.0000, 0.1455], 8 answerable test queries.
- Hybrid losses: 2. The JSON report lists every losing query and both rankings.

## Conditions and limits

- Valid complete comparison: true; violations: `{"duplicate_results": 0, "partial_embedding_coverage": 0, "partial_scan": 0, "scope_or_corpus": 0, "stale_revision": 0, "unstable_ranking": 0}`.
- Corpus: `11e3dc8560d7f980226dc21be7aa32bbeb99d58fcc75514d0fbfea16a0c7eace`; saved manifest: `1fb804e3b2980a2c14e99c46634511f68c1ef4e1be9d9a305cc9700b249fefef`.
- Reuse the saved state and unchanged database to preserve generated UUIDs and deterministic tie ordering. A fresh database assigns different IDs.
- One client, one warmup pass, frozen externally supplied vectors. Server retrieval time excludes authentication, queueing, serialization and transport; HTTP time includes the request/response round trip.
- Full generation-to-retrieval time and embedding cost are unmeasured. No provider was invoked.
- Container resource counters and all candidate/response measurements are recorded in JSON; unavailable measurements are null, not zero.
- The synthetic smoke corpus is not a representative language benchmark. Small category samples and bootstrap intervals do not establish production gains.
- No agent-task comparison was run. Better retrieval alone does not demonstrate better reasoning or autonomous improvement.

## Unanswerable queries and resource observations

The one fully judged unanswerable test query returned no records with BM25 and
returned records with both dense and hybrid retrieval. Those returned records
are not useful evidence. This trial does not include a calibrated abstention
mechanism or an agent answer policy.

Container CPU deltas for separate 18-query warm passes were 39.063 ms lexical,
40.965 ms semantic and 45.702 ms hybrid. They include telemetry activity. Sampled
container memory remained below 9.1 MB; its lifetime peak was 11,894,784 bytes.
These measurements are specific to this small, isolated trial and do not define
production limits, tenant quotas or a capacity claim.
