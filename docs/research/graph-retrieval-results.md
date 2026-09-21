# Graph retrieval results for QilbeeDB 0.13.0

The first reserved graph comparison completed 2,400 HTTP requests on 100 MuSiQue
questions, 2,311 documents and 3,791 externally constructed typed relations.
Balanced graph retrieval improved mean nDCG from 0.5311 to 0.5422 over hybrid v2,
but the paired interval includes zero. Queries with no labeled support increased
from 3% to 8%, and retrieval p95 increased by 26.6%. **The result does not qualify
graph retrieval as a default.** The API remains an explicit experimental option.

These are native laboratory measurements taken before the 0.13.0 release,
not production qualification or evidence of improved agent reasoning. The
[protocol](graph-retrieval-evaluation.md) describes selection, external graph
construction, isolation, metrics and reproduction. No source-paper experiment
was reproduced.

## Reserved results

All methods use ten final hits and the same scoped corpus. Vector methods reuse
the same 384-dimensional external `intfloat/multilingual-e5-small` vectors.
The public-development reserved cohort has 40 two-hop, 30 three-hop and 30 four-hop
questions. Other documents are unjudged; recall is limited to labeled supports.

| Method | nDCG@10 | Judged Recall@10 | All supports@10 | No support@10 | Retrieval p50 / p95 (ms) |
| --- | --- | --- | --- | --- | --- |
| BM25 | 0.5102 | 0.5250 | 0.12 | 0.05 | 43.6 / 48.0 |
| Exact cosine | 0.4972 | 0.5100 | 0.16 | 0.07 | 124.7 / 140.5 |
| Hybrid v1 | 0.5325 | 0.5392 | 0.16 | 0.04 | 142.8 / 159.2 |
| Hybrid v2 | 0.5311 | 0.5592 | 0.20 | 0.03 | 142.1 / 156.7 |
| Balanced graph, hybrid v2 seeds | 0.5422 | 0.5792 | 0.26 | 0.08 | 177.8 / 198.3 |
| Entity-weighted graph, hybrid v2 seeds | 0.5433 | 0.5842 | 0.27 | 0.08 | 178.8 / 204.3 |
| Balanced graph, BM25 seeds | 0.5211 | 0.5458 | 0.22 | 0.09 | 75.3 / 83.9 |
| Hybrid v2 seeds, depth zero | 0.5311 | 0.5592 | 0.20 | 0.03 | 142.7 / 159.9 |

Depth zero reproduced hybrid v2's exact ordered IDs for all 100 queries. All
graph methods used anchors identical to the independent seed baseline. Three
observations per query/method produced identical rankings and path proofs.
All 2,400 measured requests passed source, scope, revision, embedding, profile
and path checks. Both memory and relation histories were unchanged, and every
source and assertion was revalidated after measurement.

## Uncertainty and regressions

Balanced hybrid graph minus hybrid v2 is the predeclared primary contrast. Its
absolute nDCG delta is **+0.01112**, with a 95% paired query bootstrap interval
of **[-0.01069, +0.03283]**: 27 wins, 17 losses and 56 ties. Judged recall improves
by 0.0200 with interval [-0.02583, +0.06667]. Retrieving every labeled support
improves by 0.0600 with interval [-0.0100, +0.1300]. None of these intervals
establishes a reliable positive primary gain in this cohort.

The exploratory nDCG contrast against exact cosine is +0.04501
[+0.02061, +0.06865], while the contrast against BM25 is +0.03204
[-0.01369, +0.07621]. These additional comparisons have no multiplicity
correction and do not override the primary result or individual regressions.
The small observed entity-profile advantage is not a qualified profile selection.

| MuSiQue category | Queries | Hybrid v2 nDCG | Balanced graph nDCG | Hybrid v2 all supports | Balanced graph all supports |
| --- | --- | --- | --- | --- | --- |
| 2hop | 40 | 0.6073 | 0.6574 | 0.3000 | 0.5000 |
| 3hop1 | 23 | 0.4959 | 0.4628 | 0.1739 | 0.1304 |
| 3hop2 | 7 | 0.7415 | 0.7213 | 0.4286 | 0.2857 |
| 4hop1 | 19 | 0.4018 | 0.4009 | 0.0526 | 0.0000 |
| 4hop2 | 6 | 0.3738 | 0.3342 | 0.0000 | 0.0000 |
| 4hop3 | 5 | 0.4698 | 0.5223 | 0.0000 | 0.2000 |

The largest nDCG loss is query `3hop1__90327_73181_68042`: hybrid v2 retrieves
two of three labeled supports (nDCG 0.3084), while balanced graph retrieves none.
Query `3hop1__756602_831637_91775` falls from all three supports to one.
These are preserved regression cases, not omitted outliers. Gains on two-hop
questions do not justify an average-only promotion decision.

## Work, coverage and cost

Source and embedding scans cover all 2,311 current documents in every request.
Both hybrid profiles still cap each channel at 100 candidates; every hybrid query
reports candidate truncation. A complete source scan does not imply an exhaustive
combined ranking.

Balanced hybrid graph reports a traversal cut on 93 queries: 93 depth boundaries,
59 node limits, 57 adjacency scan limits and 15 edge limits. Reasons overlap.
Its maxima are 128 graph nodes and 1,024 examined adjacency positions. These
explicit cuts are part of the frozen bounded method; no claim covers all graph
components. Depth-zero reports 96 intentional depth boundaries and no node,
edge or scan exhaustion. The JSON preserves per-query counters and reasons.

| Method | HTTP p50 / p95 (ms) | Estimated embedding + HTTP p50 / p95 (ms) | Response p95 (bytes) | Payload p95 (bytes) |
| --- | --- | --- | --- | --- |
| BM25 | 44.6 / 49.0 | 44.6 / 49.0 | 15,764 | 12,416 |
| Exact cosine | 125.8 / 141.5 | 137.9 / 153.6 | 22,654 | 12,914 |
| Hybrid v1 | 143.9 / 160.4 | 156.0 / 174.2 | 24,459 | 12,495 |
| Hybrid v2 | 143.4 / 158.5 | 155.4 / 173.0 | 24,470 | 12,718 |
| Balanced graph, hybrid v2 seeds | 179.0 / 199.6 | 190.0 / 212.2 | 43,407 | 13,289 |
| Entity-weighted graph, hybrid v2 seeds | 180.1 / 205.9 | 190.2 / 218.6 | 43,048 | 13,289 |
| Balanced graph, BM25 seeds | 76.4 / 85.1 | 76.4 / 85.1 | 28,650 | 13,432 |
| Hybrid v2 seeds, depth zero | 143.9 / 161.2 | 156.2 / 171.9 | 35,449 | 12,718 |

Path evidence increases response size beyond the ten selected record payloads.
Intermediate record payloads are not silently added to context. Estimated combined
latency adds a previously measured query encoding time to each HTTP observation;
it is not a simultaneous end-to-end measurement. Model loading and downloading
are excluded. Document encoding took 100.15 seconds across 2,311 inputs; encoding
all 130 development/reserved questions took 1.55 seconds. No input exceeded the
512-token limit. The recorded policy allowed explicit right truncation, although
none occurred. Document/embedding/relation import and verification took 16.55
seconds. The initial graph-construction runtime was not measured separately.

No paid embedding provider was called. Local hardware and energy costs are
unmeasured. During the interleaved campaign the server consumed 303.86 CPU seconds
and reached 123.1 MiB sampled RSS. One-second sampling can miss peaks, and these
whole-process totals cannot attribute CPU to an individual method.
The dedicated database occupied 80.4 MiB of allocated filesystem space after
graceful shutdown, including all source/vector/relation data, receipts, history,
indexes and identity metadata. This is not a graph-only storage amplification
measurement. [Preparation and environment evidence](https://github.com/aicubetechnology/qilbeeDB/blob/main/benchmarks/retrieval/musique-graph-environment.json)
records these costs and confirms laboratory shutdown and credential-file removal.

## Frozen environment and audit

Measurement ran on September 21, 2026, 08:12:12–08:17:58 UTC, using a release-built
native ARM64 server on macOS, Apple M1 Pro, 10 logical CPUs and 16 GiB RAM. The
same machine continued running its existing services; this is observational
workstation latency, not an enterprise SLA. The client used Python 3.14.0,
one active request and retained database/OS caches. A fresh private laboratory
database used the real platform router and production storage configuration.
Production and the shared local Docker were unchanged.

The [public JSON report](https://github.com/aicubetechnology/qilbeeDB/blob/main/benchmarks/retrieval/musique-graph-report.json)
contains all 800 query/method rankings, sparse judgments, 2,400 timing samples,
work counters, coverage, category metrics, bootstrap comparisons and hashes of
the retained full proofs. Source text, vectors and access credentials are excluded.
The exporter recalculates per-query metrics and aggregate summaries and refuses
failed, incomplete or duplicated observations.

| Artifact | SHA-256 |
| --- | --- |
| Frozen source, canonical JSON | `175a05f5c2e1b1baaf165e17101afcc626524ac2678f352518b1e769adc642a7` |
| Frozen vectors and source, canonical JSON | `000815eaaf3d155b5b0f8c6a5d215c3a3c07a13876bc52348aab3e255cbec697` |
| Document-only relation list, canonical JSON | `3e877bcac6ec9f21e696d27d737ef5ec37d40393feb2e9f5bb5dc4487d922e2e` |
| Executed laboratory binary | `e81c657e3e7737b5a107518daf428f3a4a6f87df33c055cd4f3e3b9328c1515d` |

The E5 source revision is `614241f622f53c4eeff9890bdc4f31cfecc418b3`;
the report binds the full preprocessing identity and artifact hashes. This
experiment does not isolate dimensionality, compare embedding providers, measure
abstention or qualify consolidation. Its results remain separate from the earlier
SciFact and small memory regressions.

## Next acceptance gate

The evidence motivates investigating incorrect or overly broad document links,
competition between seed and path evidence, and coverage under high degree.
Those are hypotheses, not established causes or fixes. Any subsequent policy
must have a new server version, development-only selection and fresh reserved
queries. Keep these losses as explicit regressions. Do not erase uncertainty by
tuning against this published cohort and calling it independent confirmation.

After a retrieval policy passes that gate, run controlled agent tasks with fixed
model, prompt, tools and context budget. Measure task completion, repeated errors,
calls, tokens, latency and observed effects before claiming autonomous improvement.
