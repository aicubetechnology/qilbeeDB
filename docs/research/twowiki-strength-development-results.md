# 2Wiki path-strength development results

The frozen path-strength profile improved retrieval over the predeclared hybrid
V1 baseline on these 40 development questions. This is development evidence, not
confirmation, a new default or demonstrated improvement in agent tasks. The 120
reserved questions were not queried in this run. The existing balanced graph
profile had a slightly higher mean nDCG than strength.

## Frozen comparison

The [protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/twowiki-development-evaluation/benchmarks/retrieval/twowiki-strength-development-v1.json)
binds the exact source, graph, external vectors, model space, query roles and
seven methods. All graph methods use hybrid V1 seeds. Strength retains its
previous formula and weights. Forty questions, seven methods and three measured
repetitions produce 840 HTTP samples and 280 query/method rows. Fixed warm-up
calls are excluded from metrics. Requests are serial with retained OS/database
caches; this is not a cold-cache or saturation benchmark.

The [machine-readable report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/twowiki-development-evaluation/benchmarks/retrieval/twowiki-strength-development-report.json)
contains per-query rankings, judgments, work/coverage, costs and paired comparisons.
It comes from the [replayed corpus](twowiki-corpus-materialization.md): 1,148
documents and 328 document-derived relations. Selection is overlap-controlled
and category-stratified, not uniform or proven statistically independent.

Embeddings were generated outside the database using the previously fixed
`intfloat/multilingual-e5-small` revision and E5 mean-pooling/L2 pipeline: 384
dimensions, local CPU execution, with the exact space identity recorded in the
protocol. The 512-token right-truncation policy affected 27 documents and no
questions. All vector methods reuse identical vectors. This is not a comparison
of embedding dimensions or encoder quality.

## Retrieval results

Every method returns at most ten records. Recall is recall of the sparse positive
judgments, not exhaustive corpus relevance. Complete-support counts require all
labeled supporting documents in the top ten.

| Method | nDCG@10 | Judged recall@10 | Complete supports | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| Lexical BM25 | 0.7192 | 0.7375 | 18/40 | 23.20 |
| Semantic cosine | 0.8081 | 0.7875 | 21/40 | 65.48 |
| Hybrid V1 | 0.7722 | 0.7500 | 18/40 | 73.28 |
| Hybrid V2 | 0.8097 | 0.7875 | 21/40 | 73.56 |
| Balanced graph, V1 seeds | 0.8770 | 0.9563 | 36/40 | 72.93 |
| Strength graph, V1 seeds | 0.8738 | 0.9563 | 36/40 | 72.70 |
| Strength with depth zero | 0.7722 | 0.7500 | 18/40 | 74.46 |

The predeclared strength-minus-V1 nDCG difference is **+0.1016**, with an
exploratory paired-query bootstrap 95% interval of **[+0.0693, +0.1402]**:
19 wins, zero losses and 21 ties. Judged recall differs by +0.2063, with interval
[+0.1436, +0.2813]. Complete-support retrieval improves in 18 questions and falls
in none. Intervals use 2,000 paired query resamples and are not adjusted for
multiple comparisons; three repetitions do not create 120 independent questions.

No method returned zero labeled support on these answerable development questions.
That observation is not evidence of correct abstention on unanswerable questions.
Depth zero matches V1 in relevance here. Balanced slightly exceeds strength in
mean nDCG, so this run does not establish strength as the best graph profile.

### Categories

Each category contains ten questions. Category names retain the 2Wiki definitions;
they are not MuSiQue hop strata.

| Category | Hybrid V1 nDCG@10 | Strength nDCG@10 |
| --- | ---: | ---: |
| Bridge comparison | 0.6852 | 0.8563 |
| Comparison | 1.0000 | 1.0000 |
| Compositional | 0.7020 | 0.8424 |
| Inference | 0.7015 | 0.7964 |

## Work, coverage and cost

Every hybrid/graph query reports candidate truncation under the fixed candidate
budgets. The full source corpus being imported does not make its ranking exhaustive.
Strength and balanced report graph cuts in 1/40 questions; depth zero reports
33/40, as its traversal restriction is deliberate. Inspect individual coverage
and work counters rather than treating these cuts as missing imported documents.

Strength retrieval p95 is 72.70 ms versus V1's 73.28 ms. This small difference on
a shared workstation is not a demonstrated speed improvement. HTTP p95 is
74.09 ms versus 75.48 ms. Estimated embedding-plus-HTTP p95 is 83.10 ms versus
84.44 ms, combining previously measured external query generation with HTTP time;
it is not a fresh end-to-end provider measurement. Response-size p95 grows from
25,873 bytes for V1 to 39,969 for strength, including graph evidence.

The entire server process used 51.75 CPU seconds during measurement, with a sampled
peak RSS of 75,563,008 bytes. These process-wide observations are not per-method
attribution or capacity limits. Local compute and energy costs are unpriced; no
paid embedding-provider call was made.

## Validation and next decision

All measured calls completed; source and relation checks and change-feed fences
remained consistent. Offline reconstruction verified graph-hit evidence and seed
baselines, and the exporter reproduced aggregate metrics from recorded rows.
The report contains no reserved-query results. Experimental environment, sparse
judgments, selection exclusions, alias gaps, document truncation and graph-policy
limitations remain material.

Keep the formula and methods unchanged for reserved confirmation. If development
motivates a formula or selection change, record it and obtain a new protocol
agreement before opening reserved results. A confirmed retrieval gain would still
require separate fixed-model agent tasks to establish task quality, token savings
or improved reasoning. No such downstream conclusion follows from this run.
