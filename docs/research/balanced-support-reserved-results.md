# Balanced graph retrieval: overlap-controlled reserved comparison

The predeclared balanced graph profile with hybrid v1 seeds improved average
nDCG and complete-support retrieval on this cohort, but the primary nDCG interval
includes zero and queries with no labeled support increased. This result does
not justify changing the default or claiming a reliable overall quality gain.

The [frozen protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/support-reserved-evaluation/benchmarks/retrieval/graph-balanced-v1-support-reserved-v1.json)
and [verified report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/support-reserved-evaluation/benchmarks/retrieval/musique-balanced-v1-support-reserved-report.json)
cover 140 previously unmeasured questions, 3,175 documents and 6,257 document-only
relations. Six methods ran three repetitions, producing 2,520 HTTP measurements
and 840 query/method rows. The 30 historical development questions are excluded
from measured results; one fixed development query warms each method.

The candidate, baselines, query IDs, selection/source digests and work limits
were fixed before retrieval in tool revision `34499b0`. The qualified engine
binary was reused from source `0ba7549`; these results do not qualify a new engine
build. External multilingual E5-small vectors have 384 dimensions and were frozen
once for all methods. One document was truncated at 512 tokens; no query was
truncated. Sources, relations, history fences, native score/path evidence, the
complete matrix and recalculated aggregates passed verification. The exporter
rejected rehashed changes to the candidate, budget, provenance and query list.

## Results

Support counts refer only to original labeled supporting paragraphs. Other
corpus paragraphs are unjudged; absence of a labeled support does not prove that
the returned text is useless.

| Method | nDCG@10 | Judged Recall@10 | All supports (of 140) | No labeled support (of 140) | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `lexical` | 0.5624 | 0.5833 | 30 | 6 | 71.42 |
| `semantic` | 0.5914 | 0.6107 | 43 | 10 | 179.48 |
| `weighted_rrf_v1` | 0.6153 | 0.6369 | 39 | 5 | 216.74 |
| `weighted_rrf_v2` | 0.6194 | 0.6429 | 44 | 5 | 210.06 |
| `graph_hybrid_balanced` | 0.6324 | 0.6726 | 58 | 11 | 274.39 |
| `graph_hybrid_depth_zero` | 0.6153 | 0.6369 | 39 | 5 | 216.63 |

The primary graph-minus-hybrid-v1 nDCG difference is +0.01709, with exploratory
95% paired-query bootstrap interval [-0.00544, +0.03896]: 33 wins, 26 losses and
81 ties. Judged recall increases by 0.03571, with interval [-0.01371, +0.08214].
Neither interval excludes zero. Hybrid v2 has a higher standalone mean than v1
on this cohort; the predeclared baseline was not replaced after observation.

Complete labeled-support retrieval increases from 39 to 58 queries: a difference
of 0.13571, with exploratory interval [0.05714, 0.21429]. This secondary outcome
is promising, but does not replace the primary outcome or demonstrate agent-task
success. Three timing repetitions are not three independent relevance samples.

## Regressions and categories

Queries with no labeled support increase from five to eleven. Seven queries lose
all labeled supports present in the baseline top ten, while one recovers a labeled
support from a previously unsupported result. The worst nDCG losses are retained
in the complete public report and should become regression cases, not be hidden
by the average gain.

| Query ID | Hybrid v1 nDCG | Graph candidate nDCG |
| --- | ---: | ---: |
| `2hop__753314_23140` | 0.3978 | 0.0000 |
| `3hop1__257047_81195_59314` | 0.3296 | 0.0000 |
| `2hop__10663_24960` | 0.8503 | 0.6131 |
| `2hop__146199_89481` | 0.8503 | 0.6131 |
| `2hop__80187_59978` | 0.2372 | 0.0000 |

| Composition category | Queries | Graph candidate nDCG | Hybrid v1 nDCG |
| --- | ---: | ---: | ---: |
| `2hop` | 80 | 0.6960 | 0.6615 |
| `3hop1` | 36 | 0.5117 | 0.5225 |
| `3hop2` | 4 | 0.8226 | 0.8264 |
| `4hop1` | 12 | 0.5354 | 0.5444 |
| `4hop3` | 8 | 0.5901 | 0.5719 |

Each query has equal aggregate weight: 80 two-hop, 40 three-hop and 20 four-hop
questions. Several categories regress, and the smallest category contains only
four queries. Category means are descriptive, not separately powered conclusions.
Do not tune on this now-observed cohort and reuse it as fresh confirmation.

## Coverage, cost and provenance limits

Candidate graph traversal is cut on 132/140 queries; hybrid candidate pools are
cut on every query. Depth-zero ordering exactly matches hybrid v1, but its graph
coverage flags remain visible. Contract validation does not make bounded traversal
exhaustive. The report preserves work counters, coverage and response sizes.

Candidate retrieval p95 is 274.39 ms versus hybrid v1's 216.74 ms. These are
shared-host observations, not production capacity estimates. Estimated external
embedding-plus-HTTP time reuses measured generation costs rather than observing
simultaneous end-to-end generation. No paid provider was called; local hardware
and energy costs remain unmeasured.

The [selection and materialization procedure](disjoint-cohort-selection.md)
excludes declared question/component/answer/support overlaps against 330 historical
questions and within the new cohort. These questions come from the **public
training split**, newly reserved for this evaluation; they are not the official
hidden test set or guaranteed absent from model training. Greedy selection is
nonuniform, shared distractors and semantic equivalents may remain, and declared
disjointness is not proof of statistical independence. Bootstrap intervals are
exploratory with no multiplicity correction or guaranteed power.

No default changed. No agent task, reasoning capability or token savings were
measured. The next design decision must address the lost-support cases and be
validated separately; this report preserves the completed comparison unchanged.
