# Base-preserving graph retrieval development results

The optional `typed_path_base_preserving_v1` profile reduces the graph weight to
0.25 and increases the base weight to 0.75. This investigation tests whether that
reduces displacement of useful seed results. It does not establish a new default.

The frozen [protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/base-preserving-graph-profile/benchmarks/retrieval/graph-base-preserving-development-v1.json)
and [verified report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/base-preserving-graph-profile/benchmarks/retrieval/musique-base-preserving-development-report.json)
cover 30 public MuSiQue training questions, 2,311 documents, 3,791 document-only
relations and external multilingual E5-small vectors. Nine methods ran three
repetitions: 810 measured HTTP requests, without failures. The candidate source
was frozen at `f47d6fe9d021a3bdf10df175e34e889eb18cf77c` before retrieval. No model
was invoked. Current sources, relations, history fences, returned scores, paths,
query coverage and aggregate consistency were verified.

| Method | nDCG@10 | Judged Recall@10 | No labeled support@10 |
| --- | --- | --- | --- |
| BM25 | 0.4932 | 0.5417 | 1/30 |
| Exact cosine | 0.5382 | 0.5500 | 1/30 |
| Hybrid v1 | 0.6020 | 0.6333 | 0/30 |
| Hybrid v2 | 0.5677 | 0.5944 | 0/30 |
| Balanced graph, hybrid v2 seeds | 0.5732 | 0.6083 | 1/30 |
| Entity-weighted graph, hybrid v2 seeds | 0.5722 | 0.6083 | 1/30 |
| Balanced graph, BM25 seeds | 0.4734 | 0.4972 | 3/30 |
| Hybrid v2 seeds, depth zero | 0.5677 | 0.5944 | 0/30 |
| Base-preserving graph, hybrid v2 seeds | 0.5750 | 0.6111 | 0/30 |

The predeclared candidate-minus-hybrid-v2 nDCG difference is +0.00735, with
exploratory 95% paired bootstrap interval [-0.00206, +0.02442]: three wins, four
losses and 23 ties. Against hybrid v1, the difference is -0.02700 with interval
[-0.08965, +0.03310]. Neither comparison establishes a reliable positive gain.

In the development regression `4hop3__838995_608613_5529_4107`, hybrid v2 retrieves
three of four labeled supports, while balanced graph loses all four. The new
profile retains three supports, with nDCG 0.38702 versus hybrid v2's 0.39169.
It mitigates the severe loss but does not fully preserve ordering. The three
`4hop3` questions improve in average nDCG from 0.29476 to 0.37343; two-hop and
`3hop1` averages decline slightly. Such small categories cannot support broad
claims. Complete-support recall is unchanged overall; judgments are sparse.

All hybrid methods report candidate cuts. The candidate and balanced graph both
report graph cuts on 26/30 questions. Depth zero preserves hybrid v2 ordering.
Coverage and work must accompany relevance; a complete source scan does not mean
exhaustive candidate ranking or traversal.

This run uses an optimized native release build on a shared host. Observed
retrieval p95 is 240.85 ms for the candidate, 223.67 ms for balanced graph and
184.20 ms for hybrid v2. These are measurements from this run, not production
capacity promises or evidence of a speed improvement. External embedding times
are reused estimates, not simultaneous end-to-end observations.

## Comparison limits and record identities

The earlier development run used a debug build and a separate database import.
Fresh imports assign different record identifiers, which affect documented
UUID tie-breaks. For `2hop__861128_1136`, the same two hybrid-v1 scores tie at
0.016133229247983348; their order reverses between imports. That single question
accounts for the hybrid-v1 mean nDCG change from 0.59451 to 0.60205. It is not an
improvement to hybrid v1. Other equal-score channel orderings can also affect
fusion ranks. Compare methods within the same frozen database manifest; do not
attribute cross-import changes to the candidate or compare debug/release timing
as an algorithm improvement.

These are development results, not reserved confirmation or agent-task outcomes.
The profile remains optional and experimental. A fresh reserved protocol must be
frozen before measuring generalization, and downstream tasks need separate
controlled evaluation. No server default or existing profile was changed.
