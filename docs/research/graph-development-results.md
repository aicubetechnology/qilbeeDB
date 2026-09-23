# Graph retrieval development investigation

This investigation uses 30 public MuSiQue training questions, 2,311 documents,
3,791 document-only relations and frozen external multilingual E5-small vectors.
It completed 720 measured HTTP requests across eight existing methods and three
repetitions. The 100 previously published test questions were excluded from query
execution. This is development evidence, not a new reserved evaluation or an
agent-reasoning result.

The [protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-development-evaluation/benchmarks/retrieval/graph-multihop-development-v1.json) and
[auditable export](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-development-evaluation/benchmarks/retrieval/musique-graph-development-report.json)
retain selections, metrics, coverage and work. Source, relation and history-fence
verification passed before and after measurement. No model was invoked during
this run. Dataset attribution and CC BY 4.0 provenance are retained in the export.

| Method | nDCG@10 | Judged Recall@10 | No labeled support@10 |
| --- | --- | --- | --- |
| BM25 | 0.4932 | 0.5417 | 1/30 |
| Exact cosine | 0.5382 | 0.5500 | 1/30 |
| Hybrid v1 | 0.5945 | 0.6333 | 0/30 |
| Hybrid v2 | 0.5677 | 0.5944 | 0/30 |
| Balanced graph, hybrid v2 seeds | 0.5744 | 0.6139 | 1/30 |
| Entity-weighted graph, hybrid v2 seeds | 0.5734 | 0.6139 | 1/30 |
| Balanced graph, BM25 seeds | 0.4699 | 0.4944 | 3/30 |
| Hybrid v2 seeds, depth zero | 0.5677 | 0.5944 | 0/30 |

The primary graph-minus-hybrid-v2 nDCG difference is +0.00674, with exploratory
95% paired bootstrap interval [-0.04044, +0.05160], eight wins, seven losses and
15 ties. Against hybrid v1 it is -0.02007 [-0.08743, +0.04525]. Neither supports a
reliable positive gain or default promotion.

Two-hop mean nDCG rises from 0.6960 to 0.7375, but the eight `3hop1` queries fall
from 0.6105 to 0.5651. In `4hop3__838995_608613_5529_4107`, balanced graph retrieval
loses all labeled support and nDCG falls by 0.39169. Preserve this as a development
regression rather than hiding it behind the mean. Other subcategories contain
as few as one or two queries; their averages are not strong generalization evidence.

Depth zero preserves hybrid v2 ordering. All hybrid methods report candidate
cuts, and balanced graph reports graph cuts for 26/30 queries. Complete source
scans do not establish exhaustive candidate ranking or traversal. Sparse support
judgments limit recall to labeled evidence.

The native laboratory used an **unoptimized debug build**, and independent local
validation work overlapped parts of the run. Recorded timing and process samples
are diagnostic observations, not production capacity measurements or comparable
release-build performance claims. Estimated embedding-plus-HTTP timing reuses
previous encoding measurements; it is not simultaneous end-to-end measurement.

The next hypothesis is to reduce displacement of useful seed results while
retaining graph contributions. A new candidate needs its own immutable profile,
development comparison and a freshly reserved cohort before any quality claim.
No existing server profile or default was changed by this investigation.
