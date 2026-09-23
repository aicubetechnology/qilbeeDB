# Path-strength retrieval on the full observed regression cohort

This comparison reuses all 140 questions from the earlier balanced-profile
experiment. It is **observed regression evidence**, not an independent
confirmation. The source fixture retains its historical `test` split for
provenance; the evaluator and exported report explicitly label this use
`observed_regression`. The formula was fixed before this run and was not tuned
during measurement. No default or agent-quality claim follows from the result.

## Reproduction

The [frozen protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/path-strength-regression/benchmarks/retrieval/graph-path-strength-regression-v1.json)
and [verified observations](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/path-strength-regression/benchmarks/retrieval/musique-path-strength-regression-report.json)
cover 3,175 documents and 6,257 document-derived relations. Thirty historical
warm-up queries are excluded from measured results. Seven methods ran three
times each: 2,940 HTTP measurements and 980 query/method rows on one import.
Every graph arm used hybrid v1 seeds. The depth-zero strength control retained
the hybrid-v1 ordering, and graph anchors matched independent baseline results.

Tool source `8531cb9` was frozen before retrieval. The qualified runtime from
`bf56e34`, including path-strength feature `b0fca072`, was reused without an
engine change. All methods shared frozen external multilingual-e5-small vectors
with 384 dimensions, exact source revisions, scope, result size and work budgets.
No embeddings were generated inside the database. The earlier generation's
truncation and identity details remain in the machine-readable report.

The evaluator checked source and relation history fences before and after the
run, reconstructed path strengths and contributions, and retained work/coverage
and timing samples. The post-run audit checked frozen files, all graph proofs,
controls, aggregate metrics and support transitions. A later independent
comparison must exclude these observed queries and the declared component,
answer and supporting-document overlaps.

## Measured results

Judged recall uses positive labels and is not exhaustive corpus recall. Complete
support counts queries retrieving every positive label. No-support counts mean
no positive label was retrieved, not that every other document is irrelevant.
Latency is the server retrieval duration on a shared ARM64 developer workstation
with retained caches and one benchmark client, not production capacity evidence.

| Method | nDCG@10 | Judged recall@10 | Complete support / 140 | No labeled support / 140 | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `lexical` | 0.562419 | 0.583333 | 30 | 6 | 75.82 |
| `semantic` | 0.591415 | 0.610714 | 43 | 10 | 192.11 |
| `weighted_rrf_v1` | 0.614952 | 0.640476 | 40 | 5 | 224.79 |
| `weighted_rrf_v2` | 0.619462 | 0.642857 | 44 | 5 | 223.79 |
| `graph_hybrid_balanced` | 0.629941 | 0.670833 | 58 | 11 | 287.51 |
| `graph_hybrid_strength` | 0.639226 | 0.694048 | 58 | 6 | 286.43 |
| `graph_hybrid_depth_zero` | 0.614952 | 0.640476 | 40 | 5 | 227.58 |

The primary path-strength versus hybrid-v1 nDCG difference was
**+0.024274**, with exploratory paired-query bootstrap interval
**[+0.005581, +0.041852]**: 32 wins,
25 losses and 83 ties. This interval describes
paired variation in an already observed cohort. It does not undo adaptive
reuse, shared components or multiple development comparisons. Repeated requests
measure operational variation, not independent relevance observations.

## Lost and recovered support

Each transition compares rankings on this same import. Lost-all means the
baseline retrieved at least one positively labeled support and the candidate
retrieved none. First-support recovery is the reverse. Complete-support changes
require the full positively judged set. Exact query IDs and lost, gained and
retained document IDs are recorded in `support_transitions` in the JSON report.

| Candidate vs baseline | Lost all support | Recovered first support | Lost complete support | Gained complete support |
| --- | ---: | ---: | ---: | ---: |
| `graph_hybrid_strength` vs `weighted_rrf_v1` | 2 | 1 | 6 | 24 |
| `graph_hybrid_strength` vs `graph_hybrid_balanced` | 0 | 5 | 3 | 3 |
| `graph_hybrid_balanced` vs `weighted_rrf_v1` | 7 | 1 | 8 | 26 |

These counts are recomputed from frozen judgments and recorded rankings; a
changed summary and query list cannot silently remove a regression. The earlier
seven selected cases are not a replacement for this complete cohort, and their
numbers are not pooled across imports. UUID tie-breaks and bounded traversal
can change after reimport, even with unchanged source texts.

The exploratory interval is above zero on this observed cohort, but this is
not independent confirmation after development and diagnosis on these data.
Compared with hybrid v1, the new profile loses all labeled support on two
queries and recovers first support on one. Balanced graph loses all support on
seven and recovers one. Against balanced graph, strength recovers first support
on five queries without introducing a new no-support case in this cohort.
Both graph methods retrieve complete support on 58 queries, with three losses
and three gains between them. The `4hop1` category remains below hybrid v1.

The path-strength p95 is about 27% above hybrid v1 on this shared host. Graph
neighborhoods are cut on 132 of 140 strength queries; every hybrid/graph arm has
candidate cuts. The positive observed relevance difference does not remove
these operational costs or coverage limits.

## Limits and decision

Read the per-category scores, coverage cuts, work counts, response/payload sizes,
external-embedding-plus-HTTP estimates and process observations in the report.
Complete source scans do not imply exhaustive candidate or graph ranking.
Sparse labels leave unjudged relevance unknown. Other workstation activity and
cache conditions constrain latency comparisons.

The evidence supports only the measured development/regression findings. Keep
path-strength optional. A new candidate must be frozen before an independent
comparison, and agent-task effects need the separately agreed evaluation with
fixed model, prompt, tools and acceptance criteria. This run changes neither
production defaults nor the downstream agent method.
