# Audit retrieval evidence and preserve ranking regressions

The completed 0.9.0 SciFact campaign supports keeping hybrid v1 experimental.
It improves judged ranking quality over BM25, but does not demonstrate an nDCG
gain over exact cosine. It also recovers fewer judged-positive sources and has
higher measured retrieval cost. Correcting candidate coverage or HTTP schemas
does not establish a ranking improvement.

This guide records the integration team's findings, an independent offline audit,
and the regression gates for subsequent changes. It introduces no ranking profile,
changes no retrieval defaults and makes no downstream agent-capability claim.

## Evidence and independent checks

The source is the integration team's [completed SciFact report](https://github.com/aicubetechnology/qilbee-ecosystem/blob/2d756755321d23f2769c1e2e57110b3e25d3bfef/docs/qilbeedb-scifact-results.md)
and its curated query evidence. The campaign used all 5,183 public SciFact documents,
50 development queries, 300 test queries and frozen 1,536-dimensional external
`text-embedding-3-small` vectors. No new provider calls were made.

The [frozen regression input](https://github.com/aicubetechnology/qilbeeDB/blob/7306ee407131501a0d6a7f47e1bcb196a82ea3a0/benchmarks/retrieval/scifact-090-regression-input.json)
contains public document identifiers, sparse judgments, rankings, timing samples,
selected score contributions and original file hashes. It contains no document
texts, vectors or credentials. Dataset attribution remains the public
[BEIR SciFact distribution](https://github.com/beir-cellar/beir) and
[SciFact](https://github.com/allenai/scifact); the source report identifies the
CC-BY-SA-4.0 dataset distribution license.

Run the independent, standard-library-only audit from the repository root:

```bash
python3 scripts/audit_retrieval_evidence.py \
  --input benchmarks/retrieval/scifact-090-regression-input.json \
  --previous benchmarks/retrieval/scifact-openai1536-report.json \
  --report /tmp/scifact-regression-audit.json
```

Choose a new output path: the command refuses to overwrite an existing report.
It makes no HTTP, embedding-provider or model calls. It verifies the complete
query/method matrix, split and method identities, three timing samples per pair,
nonnegative finite measurements, duplicate-free rankings, sparse-judgment
semantics, per-query and aggregate metrics, latency quantiles, paired comparisons,
bootstrap intervals and the selected cases' RRF contributions. Altered metrics,
incomplete pairs or incompatible embedding identities fail the audit.

All **1,050 query/method pairs** passed. The checked-in
[audit result](https://github.com/aicubetechnology/qilbeeDB/blob/7306ee407131501a0d6a7f47e1bcb196a82ea3a0/benchmarks/retrieval/scifact-090-regression-audit.json)
binds the exact input hashes. Its nDCG implementation is independent of the
original evaluator: gain is `2^grade - 1`, discount is `log2(rank + 1)`, and ideal
DCG comes from the available judgments. Recall counts judged-positive sources;
it is not exhaustive recall. Nonjudged sources are not proven irrelevant.

This verifies the archived calculations, not the original HTTP traffic, source
truth or isolation. The campaign's reported zero protocol violations remain
attributed to its original harness. Likewise, the reported 44 queries with exact
hybrid ties, including 11 sensitive to judged quality, cannot all be independently
recomputed from the curated top-ten rows: only selected cases include scores.
Candidates below rank ten are not reconstructed by this audit.

## Confirmed 0.9.0 results

| Method | nDCG@10 | Judged Recall@10 | No judged-positive source in top ten | Retrieval p95 |
| --- | ---: | ---: | ---: | ---: |
| BM25 | 0.661676 | 0.790944 | 57 / 300 | 233.57 ms |
| Exact cosine | 0.724154 | 0.854889 | 41 / 300 | 1,284.56 ms |
| Hybrid v1 | 0.723702 | 0.834556 | 45 / 300 | 1,462.90 ms |

Hybrid v1 rescues 12 queries with no judged-positive cosine hit, but loses such
hits on 16 others. Against BM25 the corresponding counts are 18 rescued and six
lost. Its mean nDCG difference from cosine is -0.000453, with exploratory paired
95% interval [-0.027110, +0.025133]. This establishes neither improvement nor
equivalence. The audit reproduces the original 2,000 bootstrap draws and seed 212;
there is no correction for multiple comparisons.

Timing used one client, warm caches, three repetitions and a shared workstation.
Vectors were already available. Repetitions do not create additional independent
relevance queries. The separate 350-query resource pass reported approximately
1,122 ms of container CPU per hybrid query, 969 ms for cosine and 199 ms for BM25.
CPU includes telemetry; lifetime peak memory is not a method-specific peak. These
observations are not a service-level objective or a six-client capacity result.

## Reconcile the earlier campaign

The [0.6.0 report](scifact-results.md) is a separate campaign with two clients and
one timing observation per pair. It included both v1 and v2; the new 0.9.0 campaign
measured v1 only. Therefore, the new report neither replicates nor refutes v2's
previous result. Schema conformance is not evidence of v2 relevance.

Comparing public ranked identifiers for the same 300 test queries gives:

| Method | Changed order | Changed top-ten membership | nDCG changes above numerical tolerance |
| --- | ---: | ---: | ---: |
| BM25 | 0 | 0 | 0 |
| Exact cosine | 0 | 0 | 0 |
| Hybrid v1 | 22 | 3 | 4 |

For `test-237`, `test-507` and `test-1020`, nDCG moves from 0.630930 to 1.0;
`test-1019` moves the other way. Each of those four changes only swaps documents
with exactly equal scores in the earlier ranking. Together they account for the
mean movement from 0.721241 to 0.723702. Differences within `1e-12` are treated as
numeric noise. The three membership changes do not change judged nDCG or recall;
unseen boundary candidates prevent a complete reconstruction of their cause.

V1 breaks equal fused scores by generated UUID. New IDs can therefore change
quality metrics without changing its formula. Equal-score swaps explain the
observed aggregate movement; the comparison is not a controlled attribution of
every cross-version difference to UUIDs. Do not compare the two campaigns' latency
as an implementation speedup: their concurrency and measurement protocols differ.

## Regression cases to retain

| Query | Judged-positive source | Preserved observation | Follow-up required |
| --- | --- | --- | --- |
| `test-1088` | `37549932` | Cosine rank 1; absent from hybrid top ten | Capture full channel candidates and fused rank before assigning a cause |
| `test-1241` | `4427392` | Cosine rank 1; absent from hybrid top ten | Capture full channel candidates and fused rank before assigning a cause |
| `test-800` | `22543403` | Cosine rank 1; absent from hybrid top ten | Capture full channel candidates and fused rank before assigning a cause |
| `test-478` | `14767844` | Cosine rank 1; hybrid rank 2 after an exact fused tie | Test swapped-rank fusion and reimported IDs |
| `test-324` | `2014909` | Hybrid rank 1 from lexical rank 4 and semantic rank 13 | Preserve this complementary-channel gain |

The frozen input retains all 15 query/method rows and their available scores.
An absent returned position is **unknown beyond the top ten**, not proof that a
source was missing from the 100-candidate channel list. Changing the tie-break
alone cannot recover a source that loses by a strictly lower fused score.

Two native storage regressions exercise the mechanisms without claiming semantic
quality for synthetic vectors. One creates two independent record populations,
assigns the same content/vector pairs to opposite UUID orders, verifies v1's exact
swapped-rank tie and checks stable behavior after restart. It also confirms v2's
existing preference in this specific two-record fixture. The other places the
best cosine source in only one channel and shows that it can fall below ten
strictly higher, untied dual-channel scores despite complete source coverage and
no candidate truncation. This demonstrates why all fusion losses cannot be
attributed to ties; it does not establish the cause of every SciFact loss.

Keep the previously exposed [small-corpus diagnostic](scifact-results.md#previously-exposed-40-source-diagnostic)
as a separate regression. Its revoke/rotate paraphrase also exhibits swapped
ranks. Its one unanswered query returns no BM25 hits but returns ten cosine/hybrid
neighbors at threshold -1. A returned neighbor or RRF score does not establish
evidence sufficiency; one query cannot estimate agent error or calibrate abstention.

## Acceptance gates for subsequent changes

These 300 test queries are now exposed regression data. A profile selected after
examining their failures requires a new, independent reserved query set. Freeze
the method identity, parameters, normalization and tie-break after development,
before evaluating that set. Keep the existing v1/v2 contracts immutable.

For a fusion change, retain losses and gains above, exact identifiers, error codes,
Portuguese queries, ambiguous phrases, hard negatives and unanswered questions.
Record full channel positions and truncation. Test both stable IDs and independent
reimports. Report nDCG, judged recall, no-positive-hit counts, paired uncertainty
and category regressions. Do not tune to these five examples or publish a claimed
gain from reordered known judgments.

For a retrieval-work change, keep the same 1,536-dimensional vectors, corpus,
queries, filters, budgets and final result size. Compare clean and accumulated
history at increasing corpus sizes, first with one client and then with at least
six simultaneously active clients in a separate controlled run. Record actual
retrieval admission limits, offered and completed throughput, successful latency,
503/retry counts, failures, CPU, memory and logical bytes. Failed or retried
requests must not disappear from the load report. An admission burst is not a
sustained six-client workload. Exact retrieval must preserve results; an approximate
method requires a distinct identity and a measured recall comparison.

The [0.10.0 coverage qualification](retrieval-history-report.md) already completes
the requested three history cycles with 5,183 current records. It also records the
cost of the current-record projection on clean data. This does not close the work
on fusion quality, clean-corpus read cost or concurrent capacity. Consumer and
agent-task validation remain separate from retrieval metrics.
