# Path-strength graph retrieval: development results

The optional `typed_path_strength_v1` profile had a higher mean nDCG@10 than
hybrid v1 on this development sample, but its uncertainty interval includes zero.
It also increased local retrieval latency and lost ranking quality on five
queries. This result does **not** admit a new default or demonstrate agent
improvement.

## Frozen comparison

The [protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/path-strength-evaluation/benchmarks/retrieval/graph-path-strength-development-v1.json)
was frozen with evaluator source `bf56e34` before the HTTP run. The server was
built from that revision, including engine feature `b0fca072`. The run used
2,311 documents and 3,791 document-derived relations, one shared import,
externally generated 384-dimensional multilingual-e5-small vectors and the same
30 previously observed MuSiQue training development queries for every method.
Exact model and artifact identities are in the
[machine-readable report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/path-strength-evaluation/benchmarks/retrieval/musique-path-strength-development-report.json).

Eight methods ran three times each, yielding 720 HTTP measurements and 240
query/method rows. All graph arms used hybrid v1 seeds. Each response was limited
to ten memories; budgets, frozen revisions and scope were identical across arms.
The path-strength depth-zero control had to reproduce hybrid v1's order, and
all graph anchors had to match the separately retrieved baseline. The evaluator
reconstructed proof strengths and contributions from frozen relations and vectors.
Source and relation fences were unchanged at completion. A post-run audit replayed
graph proofs and checked frozen file hashes, aggregate metrics and completeness.

## Results

The complete-support column counts queries retrieving every positively labeled
support. Judgments are sparse; judged recall is not exhaustive corpus recall.
Latency is the server's measured retrieval duration on a shared ARM64 developer
workstation, with retained caches and one benchmark client. Other development
work was observed on the host. It is not isolated capacity or production evidence.

| Method | nDCG@10 | Judged recall@10 | Complete support / 30 | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| `lexical` | 0.493171 | 0.541667 | 4 | 51.45 |
| `semantic` | 0.538192 | 0.550000 | 7 | 127.58 |
| `weighted_rrf_v1` | 0.604098 | 0.633333 | 7 | 149.35 |
| `weighted_rrf_v2` | 0.567218 | 0.594444 | 7 | 153.89 |
| `graph_hybrid_balanced` | 0.622165 | 0.680556 | 10 | 204.43 |
| `graph_hybrid_best_channel` | 0.613638 | 0.655556 | 8 | 190.08 |
| `graph_hybrid_strength` | 0.626663 | 0.680556 | 9 | 191.37 |
| `graph_hybrid_depth_zero` | 0.604098 | 0.633333 | 7 | 152.14 |

The predeclared primary nDCG difference versus hybrid v1 was **+0.022565**, with
an exploratory paired-query bootstrap 95% interval **[-0.003122, +0.054723]**:
7 wins, 5 losses and 18 ties. Judged recall improved by +0.047222, with an interval
[-0.008333, +0.113889]. Neither interval excludes zero. The repeated HTTP samples
measure operational variation; they do not create 720 independent relevance
observations. Bootstrap intervals do not correct for repeated development use,
shared query components or multiple exploratory comparisons.

Balanced graph retrieved complete support for 10 queries versus 9 for the new
profile, despite the new profile's higher mean nDCG. The `3hop1` and `3hop2`
categories remained below the hybrid-v1 baseline. Report every category and
lost-support case when selecting a candidate; the mean alone is insufficient.
All methods returned some positively labeled support in this sample except
lexical and semantic retrieval, each of which missed it once. This does not
establish behavior on unanswerable questions.

Graph neighborhoods were cut for 27 of 30 strength queries. Every hybrid/graph
arm had candidate cuts. Complete source scans therefore do not imply exhaustive
ranking. The JSON report retains the coverage, work counts, response and payload
sizes, estimated external-embedding-plus-HTTP timing and process resource
observations needed to interpret these limits.

## Interpretation and next gate

The formula retains absolute strongest-path magnitude instead of replacing it
with a graph reciprocal rank. Its equal weights are a fixed hypothesis, not
calibrated relevance probabilities. This is not Personalized PageRank or a
reproduction of MAGMA/HippoRAG. The local native screen and the earlier selected
seven-case diagnosis are separate observations; different imports can change
UUID tie-breaks and bounded traversal, so their numbers are not pooled here.

These queries remain development data. Freeze any next candidate before an
independent comparison excluding previously observed query/component/support
history. Test both relevance and operational cost, preserve failed cases and
report uncertainty. Agent-task effects require the separate agreed evaluation
with fixed models, prompts, tools and acceptance criteria. No production setting
or downstream method changed because of this report.
