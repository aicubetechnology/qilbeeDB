# Graph retrieval with hybrid v1 seeds: development comparison

This comparison tests an existing combination: `typed_path_best_channel_v1`
with `weighted_rrf_v1` seeds. It changes no server ranking version or default.
The candidate's average nDCG exceeds its declared hybrid v1 baseline, but the
exploratory uncertainty interval includes zero. Other existing graph profiles
have higher development means in this run. No general superiority is established.

The [frozen protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-v1-seed-evaluation/benchmarks/retrieval/graph-best-channel-v1-seed-development-v1.json)
and [verified observations](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-v1-seed-evaluation/benchmarks/retrieval/musique-best-channel-v1-seed-development-report.json)
cover 30 previously observed MuSiQue training questions, 2,311 documents,
3,791 relations and external multilingual E5-small vectors with 384 dimensions.
Ten methods ran three repetitions: 900 HTTP measurements and 300 query/method
rows. All hybrid graph arms use v1 seeds; lexical graph retrieval retains lexical
seeds. Standalone hybrid v1 and v2 are both measured on this same import.

The tool revision `18607e2` was frozen before the completed run. The qualified
engine binary was reused from source `0ba7549`; this is not qualification of a
new engine build. The protocol binds seed identity, baseline, method set and
split. Responses are checked against that seed identity; depth-zero ordering and
selected anchors are compared with the independently retrieved matching baseline.
Source revisions, relation history, path/score evidence, measurement consistency,
all rows and recalculated aggregates passed verification.

| Method | nDCG@10 | Judged Recall@10 | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: |
| `lexical` | 0.4932 | 0.5417 | 50.68 |
| `semantic` | 0.5382 | 0.5500 | 128.26 |
| `weighted_rrf_v1` | 0.6020 | 0.6333 | 149.80 |
| `weighted_rrf_v2` | 0.5672 | 0.5944 | 156.50 |
| `graph_hybrid_balanced` | 0.6201 | 0.6806 | 190.86 |
| `graph_hybrid_entity` | 0.6204 | 0.6806 | 191.68 |
| `graph_lexical_balanced` | 0.4710 | 0.4944 | 88.22 |
| `graph_hybrid_depth_zero` | 0.6020 | 0.6333 | 159.79 |
| `graph_hybrid_base_preserving` | 0.6125 | 0.6583 | 193.43 |
| `graph_hybrid_best_channel` | 0.6127 | 0.6556 | 199.76 |

The predeclared best-channel-minus-hybrid-v1 nDCG difference is +0.01064,
with exploratory 95% paired-query bootstrap interval [-0.02290, +0.04799]:
seven wins, seven losses and sixteen ties. Judged recall increases by 0.02222,
with interval [-0.05556, +0.10278]. Complete labeled-support retrieval occurs in
8/30 candidate queries versus 7/30 baseline queries. These intervals do not
establish a reliable improvement.

Balanced and entity graph profiles have higher development means than the
predeclared candidate. That observation is exploratory selection evidence, not
a separate confirmed comparison. Freeze any subsequently chosen combination
before an independent evaluation; do not relabel these questions as reserved.
There is no graph-v2-seed arm in this run, so this report does not isolate the
causal effect of replacing v2 graph seeds with v1 seeds. Earlier campaigns used
different imports, whose UUID tie-breaks and bounded adjacency order can differ.

Candidate graph traversal is cut on 27/30 queries; hybrid candidate pools are
cut on all queries. Judgments are sparse original supports, so recall is judged
recall, not exhaustive recall. Query bootstrap intervals are exploratory,
uncorrected for multiple comparisons and do not account for shared components.
Agent reasoning, task completion and token savings were not measured.

An initial attempt was interrupted when review found final verifier controls
still bound to v2. Its logs are retained and its observations are excluded from
this report. After correction and adversarial tests, a launcher path error was
caught before HTTP evaluation. The completed run used the same import, retained
caches and a freshly frozen corrected runner. No ranking parameters were tuned
from the interrupted observations. This is not a cold-cache trial.

Candidate retrieval p95 is 199.76 ms versus hybrid v1's 149.80 ms. Times are
observations on a shared host, not production capacity or cross-campaign speedup
claims. External embedding-plus-HTTP estimates reuse recorded generation costs.
No provider call occurred during this comparison. The profile remains optional;
the previous v2-seed protocol and the separate agent-task protocol are unchanged.
