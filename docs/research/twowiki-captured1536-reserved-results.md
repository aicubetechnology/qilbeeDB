# Captured 1536-dimensional graph retrieval: reserved confirmation

The predeclared strength graph profile improved retrieval over hybrid V1 on the 120 reserved questions. Balanced graph retrieval achieved higher mean relevance than strength. This confirms a result for this corpus and protocol; it does not change defaults or demonstrate better agent reasoning.

## Comparison

The frozen protocol uses 1,148 documents, 328 document-derived relations, seven methods and three repetitions per question: 2,520 measured HTTP calls. The 40 development questions were excluded from measured confirmation results. All methods used the same authorized snapshot and external captured 1,536-dimensional vectors. Provider weight identity is unavailable; this is not a controlled comparison of dimensions.

The machine-readable protocol and report are `benchmarks/retrieval/twowiki-captured1536-reserved-v1.json` and `benchmarks/retrieval/twowiki-captured1536-reserved-report.json`. The protocol binds the fresh completed development report and exact input hashes. No ranking parameters changed after confirmation began.

## Results

Judged recall covers labeled positives, not exhaustive relevance. Latency is descriptive for serial requests with retained caches on a shared workstation; it is not a capacity or service-level claim.

| Method | nDCG@10 | Judged recall@10 | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: |
| lexical | 0.7724 | 0.7771 | 24.53 |
| semantic | 0.8109 | 0.7979 | 152.76 |
| weighted_rrf_v1 | 0.8105 | 0.8000 | 155.53 |
| weighted_rrf_v2 | 0.8170 | 0.8063 | 158.06 |
| graph_hybrid_balanced | 0.9105 | 0.9854 | 154.58 |
| graph_hybrid_strength | 0.8799 | 0.9417 | 157.53 |
| graph_hybrid_depth_zero | 0.8105 | 0.8000 | 163.06 |

The primary strength-minus-V1 comparison has mean nDCG gain **+0.0694**, with paired-query bootstrap interval **[+0.0534, +0.0860]**: 55 wins, 1 loss and 64 ties. The interval uses 2,000 query-level resamples at 95%, without multiplicity correction. Repetitions are timing samples, not independent questions.

## Interpretation and limits

All requests completed without contract failures, and current source and history-fence checks passed. Candidate and graph coverage limits remain part of the report; completed execution does not imply exhaustive retrieval. The benchmark uses sparse supporting-document labels and answerable questions; it does not establish abstention quality, model-unseen data, cross-tenant security coverage, or downstream task success.

Balanced retrieval was a secondary comparison and outperformed strength in mean relevance. Preserve that result and query-level losses when designing further work. These reserved results must not be reused as independent confirmation after tuning a subsequent profile. Validate any agent-task benefit separately with fixed model, prompt, tools and acceptance criteria.
