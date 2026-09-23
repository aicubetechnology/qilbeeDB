# Graph seed comparison on one import

Hybrid v1 seeds produced higher average nDCG than v2 seeds for each of three
existing graph profiles in this development run. Every exploratory uncertainty
interval includes zero. These observations do not establish a general advantage,
change a default or demonstrate improved agent outcomes.

The [frozen protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-seed-matrix/benchmarks/retrieval/graph-seed-matrix-development-v1.json)
and [verified report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-seed-matrix/benchmarks/retrieval/musique-graph-seed-matrix-development-report.json)
compare balanced, entity and best-channel graph profiles with each of hybrid v1
and v2 seeds. Two depth-zero controls and four standalone baselines complete
12 methods. The seed comparisons use the same import, 30 previously observed
MuSiQue training questions, 2,311 documents, 3,791 relations and frozen external
multilingual E5-small vectors with 384 dimensions. Three repetitions produced
1,080 HTTP measurements and 360 query/method rows.

Tool revision `71317cc` and its inputs were frozen before retrieval. The engine
binary was reused from qualified source `0ba7549`; the experiment does not
qualify the newest engine build. Every arm's seed identity, graph profile and
expansion settings were verified. Both depth-zero controls reproduced their
matching hybrid order, and graph anchors matched independent retrieval of that
same seed version. Current source and relation verification, unchanged history
fences, path/score proofs, all measurements and recomputed aggregates passed.
The exporter rejected altered, mismatched and omitted paired seed results.

| Method | nDCG@10 | Judged Recall@10 | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: |
| `lexical` | 0.4932 | 0.5417 | 51.45 |
| `semantic` | 0.5382 | 0.5500 | 131.00 |
| `weighted_rrf_v1` | 0.6020 | 0.6333 | 154.41 |
| `weighted_rrf_v2` | 0.5677 | 0.5944 | 153.49 |
| `graph_v1_balanced` | 0.6205 | 0.6806 | 197.35 |
| `graph_v1_entity` | 0.6209 | 0.6806 | 217.48 |
| `graph_v1_best_channel` | 0.6122 | 0.6556 | 202.09 |
| `graph_v1_depth_zero` | 0.6020 | 0.6333 | 170.24 |
| `graph_v2_balanced` | 0.5732 | 0.6083 | 202.49 |
| `graph_v2_entity` | 0.5722 | 0.6083 | 216.58 |
| `graph_v2_best_channel` | 0.5928 | 0.6528 | 192.40 |
| `graph_v2_depth_zero` | 0.5677 | 0.5944 | 166.38 |

## Paired seed effects

These differences compare v1 minus v2 while holding the graph profile and
import fixed. They describe the observed seed-policy effect on this cohort;
they are not comparisons between separate imports.

| Graph profile | Mean nDCG difference | Exploratory 95% interval | Wins / losses / ties |
| --- | ---: | --- | --- |
| `balanced` | +0.04728 | [-0.01442, +0.11398] | 12 / 10 / 8 |
| `entity` | +0.04870 | [-0.01023, +0.11134] | 13 / 9 / 8 |
| `best_channel` | +0.01942 | [-0.04243, +0.08631] | 11 / 10 / 9 |

The predeclared primary comparison is balanced with v1 seeds against standalone
hybrid v1. Its mean nDCG difference is +0.01842, with interval
[-0.01961, +0.05994]: nine wins, eight losses and thirteen ties. Entity's slightly
higher mean does not replace the predeclared candidate. The next confirmation
should freeze the complete candidate and new cohort before observing results.
The already observed development and reserved cohorts remain regression data.

## Limits

These 30 questions have been used repeatedly for development. Judgments cover
original supports only; recall is judged recall, not exhaustive recall. Query
bootstrap intervals are exploratory, are not adjusted for multiple comparisons
and do not account for shared question components. Three measurement repetitions
do not constitute three independent relevance samples per question. No interval
establishes a reliable gain here.

Traversal is cut on 27/30 queries in the full-depth v1 graph arms and 26/30 in
the full-depth v2 arms. Hybrid candidate pools are cut on every query. Coverage
limits remain visible even when seed and path contracts pass. Depth-zero
controls also retain their reported graph-budget limitations.

Balanced-v1 retrieval p95 is 197.35 ms versus standalone hybrid v1's 154.41 ms.
All times are observations on a shared host with retained caches, not production
capacity or a cross-campaign speedup claim. External embedding-plus-HTTP values
reuse generation timings; no new embedding provider call occurred in this run.
Rankings across different imports can change through UUID ties and bounded
adjacency order. Agent tasks, reasoning and token savings were not measured.
Existing ranking profiles, legacy protocols and defaults are unchanged.
