# Real-embedding retrieval results for QilbeeDB 0.6.0

## What was measured

This report compares lexical BM25, exact cosine and two immutable hybrid profiles
using all 5,183 SciFact documents and all 300 official BEIR test queries. Embeddings
are frozen OpenAI `text-embedding-3-small` vectors with **1536 dimensions**.
The separate 40-source diagnostic and optional E5 development study are not pooled
with this result. Neither test measures downstream agent capability.

The full campaign completed 1,200 measured HTTP requests with no recorded coverage,
scope, source-revision, embedding-receipt, duplication or profile failures. Each
request ranked a complete scoped snapshot using a 128 MiB scan budget. Both hybrid
profiles remain experimental; reported gains apply to this collection and model.

## Reserved SciFact results

| Method | nDCG@10 | Judged Recall@10 | No judged-relevant hit | Engine p50 / p95 (ms) | HTTP p50 / p95 (ms) |
| --- | --- | --- | --- | --- | --- |
| BM25 | 0.6617 | 0.7909 | 0.1900 | 190.9 / 285.1 | 196.4 / 291.0 |
| Exact cosine | 0.7242 | 0.8549 | 0.1367 | 1092.4 / 1601.7 | 1100.2 / 1619.5 |
| Hybrid v1 | 0.7212 | 0.8346 | 0.1500 | 1191.1 / 1919.3 | 1199.7 / 1927.4 |
| Hybrid v2 | 0.7355 | 0.8726 | 0.1200 | 1176.4 / 2008.7 | 1186.6 / 2034.3 |

All 300 queries have at least one positive judgment. Qrels are sparse: the recall
column counts known relevant documents, not every potentially relevant document.
nDCG uses gain `2^grade - 1` and discount `log2(rank + 1)`; SciFact positive grades
are 1 and unjudged documents contribute zero to the judged metric. No-hit rates
mean no *judged* relevant hit, not proof that every returned document is irrelevant.

## Uncertainty and individual regressions

V2 has the highest mean nDCG and judged recall in this campaign. Its nDCG gain
over cosine is **0.0113 absolute** (about **1.56% relative**), with 34 wins,
21 losses and 245 ties. The exploratory interval is positive, but this is one
collection with sparse judgments and five unadjusted comparisons. The difference
between v2 and v1 has an interval spanning zero; their mean ordering does not
establish a reliable improvement over v1. V2 also has a higher observed p95 latency
than cosine. These results support an explicit experimental option, not changing
the legacy endpoint or claiming universal hybrid superiority.

| Comparison | Mean nDCG difference | Exploratory 95% paired interval | Wins / losses / ties |
| --- | --- | --- | --- |
| Hybrid v1 − BM25 | +0.0596 | [+0.0326, +0.0866] | 80 / 31 / 189 |
| Hybrid v1 − Exact cosine | -0.0029 | [-0.0284, +0.0230] | 56 / 49 / 195 |
| Hybrid v2 − BM25 | +0.0738 | [+0.0408, +0.1069] | 90 / 50 / 160 |
| Hybrid v2 − Exact cosine | +0.0113 | [+0.0016, +0.0211] | 34 / 21 / 245 |
| Hybrid v2 − Hybrid v1 | +0.0142 | [-0.0074, +0.0358] | 50 / 47 / 203 |

Intervals use 2,000 paired resamples of queries with the frozen seed. They are
exploratory, without correction for multiple comparisons. An interval containing
zero does not establish a reliable positive gain. The [full JSON report](https://github.com/aicubetechnology/qilbeeDB/blob/main/benchmarks/retrieval/scifact-openai1536-report.json)
preserves every ranking, score contribution, candidate count and individual loss.

## Selection and experimental conditions

The first declared grid compared 20 RRF settings on 50 development queries selected
by identifier before retrieval. After inspecting development results only, an
expanded declared search compared 114 distinct RRF settings and 19 normalized-score
combinations. The original 20 RRF settings are included in the 114.
Min-max normalization used each channel’s top-100 candidates; raw BM25 and cosine
numbers were never added directly. No test result was consumed during selection.
The selected v2 profile fixes lexical/semantic weights at **0.25/0.75** and rank
constant **2**. V1 remains **0.5/0.5**, constant **60**. Both cap each channel at
100 candidates, return ten results and break ties by source UUID.

Development nDCG was 0.5660 for BM25, 0.6829 for cosine and 0.6917 for the selected RRF candidate. These are selection scores, not held-out evidence.

The held-out protocol was frozen before test execution. Two concurrent clients
processed one seeded shuffle of all 1,200 query/method jobs, with one observation
per pair. The first development query warmed each method once and was excluded.
RocksDB/OS caches were not flushed; no retrieval-result cache was used. The earlier
unexecuted sequential two-pass plan was replaced before any held-out retrieval.
Sources were verified before and after the run. The final report binds the exact
fixture, source manifest, profiles, settings and runtime image.
There is no repeated-query stability estimate. Latency describes this local Docker
run with concurrency two; it is not an enterprise SLA or directly comparable to
the earlier 40-source single-client report.

Embedding generation and model loading are excluded. Frozen vectors were reused
without provider calls. End-to-end generation latency and provider billing were
not measured in this campaign. The `captured-…` revision identifies the captured
vector set, not an immutable revision guaranteed by the remote provider.

## Complete coverage and dimensional support

The [capacity preflight](https://github.com/aicubetechnology/qilbeeDB/blob/main/benchmarks/retrieval/scifact-capacity-preflight.json)
reproduced a partial hybrid scan at 64 MiB and completed all 5,183 sources with
128 MiB. Its full hybrid scan accounted for **115,105,204 serialized bytes**.
No vector dimensionality or corpus size was reduced and no independent BM25 pages
were merged. `exhaustive` describes source-scan coverage; the documented
100-candidate channel cap still applies and `candidates_truncated` remains visible.

An eight-request burst admitted two retrievals and returned six explicit
`503 retrieval_busy` responses. A later request succeeded after permits were
released. This is a functional admission check, not a sustained load benchmark.
Separate native and Docker tests cover 1,536, **3,072**, 4,096, 8,192 and 32,768
dimensions, persistence, exact cosine, invalid dimensions and transport limits.
Those synthetic dimension tests do not measure semantic quality at larger dimensions.
See [retrieval capacity](../operations/retrieval-capacity.md) for limits and configuration.

## Previously exposed 40-source diagnostic

The team’s original real-vector fixture was replayed separately after freezing the
new profile. Its eight answerable test queries and one unanswerable query were
already exposed by the earlier report. They detect regressions but are not a new
unbiased qualification set.

| Method | nDCG@10 (8 answerable queries) | Judged Recall@10 | Return rate on the 1 unanswerable query |
| --- | --- | --- | --- |
| BM25 | 0.9637 | 1.0000 | 0.0000 |
| Exact cosine | 1.0000 | 1.0000 | 1.0000 |
| Hybrid v1 | 1.0000 | 1.0000 | 1.0000 |
| Hybrid v2 | 1.0000 | 1.0000 | 1.0000 |

The [diagnostic JSON](https://github.com/aicubetechnology/qilbeeDB/blob/main/benchmarks/retrieval/contract-openai1536-report.json)
retains each category, ranking and contribution. Inspect the exact-code and
paraphrase rows individually. All four methods rank the exact error-code target
first. For the revoke/rotate paraphrase, v1 gives both candidates the identical
score `0.01626123744050767`: their lexical ranks 1/2 and cosine ranks 2/1 cancel
under equal weights. The new source UUIDs break this tie in favor of the more
relevant candidate, unlike the earlier run. Therefore v1's change from the team's
earlier 0.9637 to 1.0000 is not a ranking-algorithm improvement. V2 distinguishes
those candidates with scores 0.3125 and 0.2708333333333333 in this run.
Returning neighbors on the unanswerable query is
not evidence sufficiency; this release does not introduce calibrated abstention.
The diagnostic sources were removed from the isolated benchmark container and its
temporary credential was revoked after source verification. Frozen vectors and
private manifests remain outside the repository; no source content or vectors
from SciFact are published here.

## Resource measurements

The JSON report contains response sizes, scanned bytes and aggregate container
CPU/RSS counters before and after the interleaved campaign. Resource use cannot
be attributed to individual methods in this run. The memory peak is since container
startup, including earlier preflight work, not an isolated per-query peak.
The host also ran its existing services; CPU/latency is observational.

## Separate E5 development study

The optional 384-dimensional E5 study used 807 development queries, with no held-out
E5 quality run in this release. Its selected development RRF setting was 0.5/0.5
with constant 5, scoring 0.7263 nDCG, versus BM25 0.6708 and cosine 0.6770.
E5 and OpenAI trials differ in models, preprocessing and development sets; these
figures cannot establish a dimensionality effect. E5 right truncation affected
731 of 5,183 documents; full text was preserved for lexical retrieval.

## Reproduce and interpret

Use the [SciFact evaluation workflow](scifact-evaluation.md), archived frozen
vectors, exact source state and pinned interleaved plan. A fresh ingestion assigns
new UUIDs and may alter exact score ties. Give large campaigns dedicated scopes;
an initial preparation attempt shared a namespace and exceeded its record-count
budget, so those newly created sources were deleted and the measured campaign
used a separate mission and private subject. No partial-scan relevance results
were included. Cleanup is terminal: use new run identities rather than reusing
deleted source receipts.

The corpus is the [BEIR SciFact distribution](https://github.com/beir-cellar/beir),
SHA-256 `536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165`,
with judgments from [SciFact](https://github.com/allenai/scifact). Preserve its
upstream attribution and licenses. The evaluation protocol and research references
are documented in the [methodology](scifact-evaluation.md).

This result does not establish multi-domain superiority, calibrated evidence
sufficiency, improvement at 3,072 dimensions or better agent tasks. Further profile
selection requires new development data and an independent reserved evaluation.
The exposed 300-query test must not become the next tuning set while retaining a
claim of independent confirmation.

## Subsequent independent integration evidence

The [0.9.0 integration audit](retrieval-regressions.md) recalculates 1,050 query/method pairs and compares the same public test rankings with this campaign. It identifies exact-tie sensitivity without treating fresh record IDs as an algorithm improvement. The campaigns have different timing protocols; their performance measurements remain separate.
