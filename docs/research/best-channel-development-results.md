# Best-channel graph retrieval development comparison

The experimental `typed_path_best_channel_v1` profile uses the maximum
reciprocal rank from the base and graph channels. It can admit graph-only
results without the top-ten barrier of the base-preserving weighted profile.
This development comparison does not establish general superiority or admit a
new default.

The [frozen protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/best-channel-evaluation/benchmarks/retrieval/graph-best-channel-development-v1.json)
and [verified report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/best-channel-evaluation/benchmarks/retrieval/musique-best-channel-development-report.json)
cover 30 previously used MuSiQue training questions, 2,311 documents and 3,791
relations. External multilingual E5-small vectors have 384 dimensions. Ten
methods ran three repetitions, yielding 900 HTTP measurements without reported
failures. No reserved query was evaluated. Source revision
`0ba7549` was frozen before measurement; the new engine profile originates from
`ba659a0`. Sources, relations, history fences, path/score evidence, the complete
300-row matrix and recalculated aggregates passed verification. Additional
measurement-consistency checks were applied after the frozen runner completed.

| Method | nDCG@10 | Judged Recall@10 | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: |
| `lexical` | 0.4932 | 0.5417 | 56.86 |
| `semantic` | 0.5382 | 0.5500 | 134.14 |
| `weighted_rrf_v1` | 0.5945 | 0.6333 | 158.97 |
| `weighted_rrf_v2` | 0.5672 | 0.5944 | 166.30 |
| `graph_hybrid_balanced` | 0.5800 | 0.6250 | 222.79 |
| `graph_hybrid_entity` | 0.5790 | 0.6250 | 205.66 |
| `graph_lexical_balanced` | 0.4653 | 0.4778 | 91.82 |
| `graph_hybrid_depth_zero` | 0.5672 | 0.5944 | 156.55 |
| `graph_hybrid_base_preserving` | 0.5752 | 0.6111 | 200.62 |
| `graph_hybrid_best_channel` | 0.5936 | 0.6528 | 203.43 |

The primary candidate-minus-hybrid-v2 nDCG difference is +0.02637, with
exploratory 95% paired-query bootstrap interval [-0.00807, +0.06275]: eight wins,
seven losses and 15 ties. Judged recall increases by 0.05833, with interval
[-0.01389, +0.13340]. Both intervals include zero. Hybrid v1 retains a slightly
higher average nDCG, while the candidate has higher judged recall. Neither
metric alone establishes better downstream task outcomes.

Candidate traversal is cut on 26/30 queries; hybrid candidate pools are cut on
all queries. Current-source verification does not make a bounded ranking
exhaustive. Judgments are sparse original supports, so recall is judged recall;
unjudged paragraphs are not proven irrelevant. The bootstrap is exploratory,
uncorrected for multiple comparisons and does not account for shared query
components. These previously used questions are development evidence only.

Observed candidate retrieval p95 is 203.43 ms versus hybrid v2's 166.30 ms.
This is a shared-host observation, not a production capacity or algorithmic
speedup claim. External embedding-plus-HTTP values reuse measured generation
times rather than observing simultaneous end-to-end generation. All methods use
the same frozen vectors and import within this campaign.

The earlier offline prototype used a different database import. UUID tie-breaks
and bounded adjacency order can change across imports; results must not be
interpreted as a direct algorithm comparison across those runs. The previous
native validation reproduced that prototype exactly on its original import.
This report records a separate HTTP comparison with current baselines.

The profile remains optional. Freeze a new independent cohort and analysis
before claiming generalization, and keep agent-task quality and token accounting
as separate evaluations. Existing ranking versions and defaults are unchanged.
