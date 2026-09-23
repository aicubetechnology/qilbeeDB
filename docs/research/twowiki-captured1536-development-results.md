# Captured 1536-dimensional graph retrieval: development results

The strength graph profile improved mean retrieval quality over the predeclared
hybrid V1 baseline, with one query-level regression. Balanced graph retrieval
had higher mean quality than strength. This is development evidence; it does not
change the production default or establish an improvement in agent reasoning.

## Reproducible comparison

The [frozen protocol](twowiki-captured1536-development-protocol.md) binds 1,148
documents, 328 document-derived relations, 40 development questions, seven
methods and three repetitions: 840 measured HTTP calls, plus fixed warm-up calls.
The 120 reserved questions were not queried. Requests were serial with retained
OS/database caches on a shared workstation, not a cold-cache or capacity trial.

All vector methods reuse the same externally captured OpenAI
`text-embedding-3-small` vectors, 1,536 dimensions, and exact snapshot identity.
The provider weight revision is unavailable. This is separate from the earlier
384-dimensional study, not a controlled test of dimension count alone.

The machine-readable evidence is
`benchmarks/retrieval/twowiki-captured1536-development-report.json`. It contains
per-query rankings, sparse judgments, coverage, response sizes and paired
comparisons, with exact protocol, fixture and generation hashes.

## Results

Recall covers labeled positive judgments, not exhaustive relevance. Complete
support means every labeled supporting document appears in the top ten.

| Method | nDCG@10 | Judged recall@10 | Complete supports | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| lexical | 0.7192 | 0.7375 | 18/40 | 27.28 |
| semantic | 0.8060 | 0.8063 | 23/40 | 217.25 |
| weighted_rrf_v1 | 0.7846 | 0.7812 | 21/40 | 229.62 |
| weighted_rrf_v2 | 0.8096 | 0.8063 | 23/40 | 212.60 |
| graph_hybrid_balanced | 0.8790 | 0.9563 | 36/40 | 227.75 |
| graph_hybrid_strength | 0.8549 | 0.9250 | 32/40 | 217.26 |
| graph_hybrid_depth_zero | 0.7846 | 0.7812 | 21/40 | 209.97 |

Strength minus V1 nDCG is **+0.0702**, with exploratory paired-query bootstrap
95% interval **[+0.0399, +0.1022]**: 19 wins, one loss and 20 ties. Complete
support improves in 12 questions and regresses in one. Intervals use 2,000 paired
resamples without multiplicity correction; repetitions are not independent queries.

The V1-to-strength regression occurs in `twowiki:2eaadcce0bb011ebab90acde48001122`.
Retain this case when assessing future profiles. Against semantic retrieval,
strength has 15 nDCG wins, eight losses and 17 ties; average gain is not universal.
Depth zero matches V1 relevance. No method has zero labeled support on these
answerable questions; this does not test abstention on unanswerable inputs.

### Categories

Each category has ten questions.

| Category | Hybrid V1 nDCG | Strength nDCG | Balanced nDCG |
| --- | ---: | ---: | ---: |
| bridge_comparison | 0.6973 | 0.7950 | 0.8738 |
| comparison | 0.9850 | 0.9850 | 0.9850 |
| compositional | 0.7001 | 0.8343 | 0.8424 |
| inference | 0.7560 | 0.8051 | 0.8147 |

## Coverage and cost

All hybrid and graph queries have candidate cuts under the frozen budgets.
Strength and balanced have graph cuts in 1/40 questions; depth zero in 35/40.
All source scans and current embeddings were complete. These facts do not make
candidate ranking exhaustive.

Strength retrieval p95 is 217.26 ms versus V1 229.62 ms; HTTP p95 is 219.31 ms
versus 230.78 ms. Shared-host variation prevents interpreting this as a proven
speed improvement. Response-size p95 grows from 24,820 bytes to 37,966 bytes.
Individual external embedding and combined embedding-plus-HTTP durations are
**unavailable**, stored as null. Batch capture recorded 131,429 tokens across
21 batches; monetary cost is unavailable and no new provider calls were made.

## Validation and limits

All calls completed. Source and relation checks and change-feed fences remained
consistent. Response validation reconstructed typed path contributions, checked
cosine and source revisions, and verified seed baselines. The offline exporter
recomputed metrics, categories and paired comparisons across all 280 rows.

Sparse judgments, document-only graph construction, cohort selection and fixed
work budgets limit generalization. Keep methods frozen for a separately agreed
reserved comparison. Any changed formula needs a new development protocol before
reserved results are opened. Fixed-model downstream agent tasks remain necessary
to establish task quality, token savings or reasoning improvements.
