# Real-container development qualification and import identity

A real ARM64 container completed the captured 1,536-dimensional development
comparison. It used the actual HTTP router, persistent database and scoped
credential. No simulated service was used. This remains development evidence,
not reserved confirmation, default admission or proof of improved agent reasoning.

## Deployment and measurement

The image was built from revision `985e1749dea6635f01c0313a0c055c5ac288a7b8`;
its digest is `sha256:adaf87120ee8e85b8e73e56aba91c8bb9c1f2bdb821181fafec78761b80114f8`.
The server reports version 0.14.0, so the version string alone does not distinguish
this candidate from earlier builds. Source and image identities are necessary.
The container ran on Linux ARM64 with a two-CPU limit and 2 GiB memory limit;
the HTTP evaluator ran on the host. The report's environment describes that client,
not the server kernel. This is not a capacity-sizing benchmark.

The [frozen development protocol](twowiki-captured1536-development-protocol.md)
remains unchanged: 1,148 documents, 328 relations, 40 development queries, seven
methods and three repetitions, totaling 840 measured calls. Fixed warm-up calls
are excluded. The 120 reserved queries were not queried. External vectors were
reused without new provider calls. Caches were retained; requests were serial.

The separate machine-readable report is
`benchmarks/retrieval/twowiki-captured1536-docker-development-report.json`.
Its canonical SHA-256 is
`ebdae489f496498974f57625b7c8c1cd23a9d489350163a17f5b0ce3f06cc3fc`.
It includes per-query rankings and sparse judgments, coverage, measured latency,
response size and paired comparisons. It does not replace the earlier report.

## Observed results

| Method | nDCG@10 | Judged recall@10 | Complete supports | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| lexical | 0.7192 | 0.7375 | 18/40 | 27.71 |
| semantic | 0.8060 | 0.8063 | 23/40 | 230.13 |
| weighted_rrf_v1 | 0.7833 | 0.7812 | 21/40 | 231.60 |
| weighted_rrf_v2 | 0.8086 | 0.8063 | 23/40 | 255.45 |
| graph_hybrid_balanced | 0.8777 | 0.9563 | 36/40 | 218.43 |
| graph_hybrid_strength | 0.8536 | 0.9250 | 32/40 | 232.25 |
| graph_hybrid_depth_zero | 0.7833 | 0.7812 | 21/40 | 228.87 |

The predeclared strength-minus-V1 nDCG difference remains +0.0702, with exploratory
paired 95% interval [+0.0399, +0.1022]: 19 wins, one loss and 20 ties. Balanced has
higher mean quality than strength. Recall covers judged positives, not all relevant
documents. All hybrid/graph rows report candidate cuts; strength and balanced
report graph cuts in one question. Source and embedding coverage were complete.

Individual embedding generation and combined generation-plus-HTTP durations remain
unavailable (null). Server CPU/RSS attribution was unavailable in this evaluator;
the single operator resource observation is not a campaign resource measurement.
Do not compare these latency observations causally with the native-host run.

## Why import identity matters

Compared with the [earlier import](twowiki-captured1536-development-results.md),
25 query/method rankings changed and five nDCG results changed. Records received
new UUIDs. The documented ranking tie-break uses ascending record UUID, so a fresh
import is not an identical ranking input even when text and vectors match.

For query `twowiki:3cf8c6e0084c11ebbd56ac1f6bf848b6`, two hybrid V1 hits both score
0.016001024065540194. Their alias order reverses with their UUID order, which also
changes graph base ranks. Another observed lexical tie changes a lexical channel
rank from nine to eight and therefore its RRF contribution. These are observed
mechanisms, not proof that every difference is attributable solely to UUIDs.

Preserve the exact import and current revisions for confirmation, bind its
manifest to the evidence, and keep cross-import results separate. Do not silently
retune tie-breaks in an immutable ranking version after inspecting results.

## Restart observation

After a graceful container stop and restart, all 280 development query/method
requests were repeated against the same persisted import. Ranked document IDs,
full hit evidence and returned relations matched the earlier Docker observations
exactly, and change-feed fences remained unchanged. These extra requests verify
restart stability; they are not added as independent relevance samples or latency
repetitions. The successful same-import replay does not establish cross-import
identity or attribute every earlier difference to a single cause.

## Acceptance boundary

All measured responses passed current-source, scoped-revision, typed-path,
cosine-evidence and repetition-stability checks; change-feed fences remained
consistent. The actual authenticated client rejected an unfrozen confirmation
protocol while the database fences remained unchanged. No reserved protocol is
activated by this report.

A reserved admission gate requires an exact completed development report and
unchanged comparison fields. It rejects changed formulas, models, budgets,
primary comparison or evidence scope, and rejects an unsupported production-default
claim. Explicit role transitions have fixed audience labels. A missing frozen
protocol fails before retrieval. Selecting a confirmation protocol remains a
separate decision; this page is not authorization to open reserved queries.
