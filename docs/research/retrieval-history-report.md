# Retrieval coverage with accumulated history

The current-record candidate projection restores complete source-corpus coverage in the measured deletion/replacement scenario without increasing the 10,000-candidate budget. These are coverage and resource measurements, not evidence that hybrid retrieval improves relevance or agent capability.

## Method and provenance

All successful runs used isolated Docker instances with 2 CPUs, a 2 GiB memory limit, dedicated host bind directories, 1,536-dimensional external fixture vectors, 20 measured requests after two warmups, one retrieval request at a time, and a 128 MiB lexical/hybrid byte budget. Mutations used four workers. The other team’s shared service was not restarted or changed.

The baseline is 0.9.0 at `4d12a1c0f14b28112cce7d8d3ab4d25a0d4471d0`. The measured candidate is 0.10.0 at `ca40867ace7a27876bd45be4aa3b8745fb1f4e73`. Exact image identities, per-method pages, CPU/RSS observations and artifact digests are in the [machine-readable report](retrieval-history-report.json).

Generated documents and vectors are synthetic. Every generated document has the same vector; this is intentionally a capacity fixture. The first volume-backed baseline failed when the Docker disk filled and was excluded. Repeated warm queries and uncontrolled workstation activity limit performance inference.

## Three update, reembedding and replacement cycles

Each cycle updates and reembeds every current record, then deletes and replaces all 5,183 records. Replacement creates new identifiers while preserving tombstones, receipts and events. Finally, 1,024 current records with another tag are added.

| Stage | Current corpus | Tombstones | 0.9.0 covered, all three modes | Candidate covered, all three modes |
| --- | ---: | ---: | ---: | ---: |
| initial | 5183 | 0 | 5183 | 5183 |
| cycle-1-updated | 5183 | 0 | 5183 | 5183 |
| cycle-1-replaced | 5183 | 5183 | 4991 | 5183 |
| cycle-2-updated | 5183 | 5183 | 4991 | 5183 |
| cycle-2-replaced | 5183 | 10366 | 3357 | 5183 |
| cycle-3-updated | 5183 | 10366 | 3357 | 5183 |
| cycle-3-replaced | 5183 | 15549 | 2548 | 5183 |
| outside-tag-background | 5183 | 15549 | 2439 | 5183 |

Updates and reembedding of the same identifier did not create additional canonical candidates. Loss of coverage began after deletion/replacement. In the legacy scan, tombstones and retained bindings consumed the budget; outside-tag records reduced eligible coverage further. The controlled UUID-prefix breakdown is fixture-derived, not production instrumentation.

All candidate stages covering 5,183 current documents examined exactly 5,183 records and reported `exhaustive: true`. Hybrid retained its existing channel caps and `candidates_truncated` reporting. Current source bytes stayed at 6,761,595 for lexical and 93,188,120 for hybrid; candidate index bytes are separate. These byte counts are not client traffic, RAM or physical I/O.

## Same database before and after migration

A stopped copy of the exact baseline database was upgraded with all 15,549 tombstones and 1,024 outside-tag records intact. Coverage rose from 2,439 to 5,183 current documents in every mode. Startup including container creation and health polling took 0.969 seconds in this fixture; this is not a general migration-time bound.

| Method | Version | Covered | Scanned candidates | Retrieval p50 / p95 (ms) | CPU seconds/query, including warmup | Ending RSS (MiB) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| lexical | 0.9.0, partial | 2439 | 10000 | 105.60 / 140.34 | 0.1164 | 61.6 |
| lexical | candidate, complete | 5183 | 5183 | 115.74 / 118.46 | 0.1186 | 77.3 |
| semantic | 0.9.0, partial | 2439 | 10000 | 1480.37 / 2252.93 | 1.6423 | 87.1 |
| semantic | candidate, complete | 5183 | 5183 | 830.08 / 1042.46 | 0.8741 | 94.7 |
| hybrid | 0.9.0, partial | 2439 | 10000 | 458.81 / 500.40 | 0.4636 | 112.0 |
| hybrid | candidate, complete | 5183 | 5183 | 873.88 / 1090.68 | 0.9109 | 155.7 |

The hybrid candidate takes longer than the partial legacy query in this comparison while covering more than twice as many current documents. Do not report this as an equivalent-work latency regression or speedup. Semantic process CPU falls while eligible coverage increases. The new path skips deleted vectors, but these totals include background server work and do not isolate that effect. Memory samples include process state accumulated before each observation.

## Clean-corpus size curve

| Current documents | Method | 0.9.0 p95 (ms) | Candidate p95 (ms) | 0.9.0 CPU s/query | Candidate CPU s/query |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1024 | lexical | 19.67 | 24.70 | 0.0186 | 0.0232 |
| 1024 | semantic | 149.36 | 200.22 | 0.1445 | 0.1900 |
| 1024 | hybrid | 162.03 | 200.12 | 0.1545 | 0.1868 |
| 2592 | lexical | 56.05 | 63.22 | 0.0495 | 0.0582 |
| 2592 | semantic | 384.20 | 477.02 | 0.3623 | 0.4545 |
| 2592 | hybrid | 417.46 | 501.12 | 0.3918 | 0.4755 |
| 5183 | lexical | 109.06 | 162.55 | 0.0955 | 0.1300 |
| 5183 | semantic | 1000.98 | 1249.18 | 0.8514 | 0.9750 |
| 5183 | hybrid | 1074.15 | 1065.90 | 0.9241 | 1.0191 |

The candidate has higher p95 in eight of the nine clean-corpus mode/size pairs and higher measured process CPU in all nine. The new projection has a cost on clean data; this result must remain visible when assessing the coverage fix. It is not a general latency improvement.

These separately executed warm runs show work at three corpus sizes. They do not control every source of workstation contention or establish a service-level objective. Full per-stage history curves and response sizes remain in the JSON report.

## Real-vector eight-document regression

The exact eight public SciFact documents and frozen 1,536-dimensional vectors from the integration report were reused, with subset SHA-256 `4a3cb0db4399cd495fa07562666e6d271c98ec90d495e123ec7539e020912ecb`. No embeddings or model outputs were generated.

After deleting the first four UUIDs in cursor order, all four survivors remained directly readable. With candidate budget four, all three modes covered four records, returned the target, reported complete coverage and required no continuation. Budget eight also examined only four current candidates. All four deletion events remained in the journal. Responses validated against the OpenAPI served by the real 0.9.0 process. New work metadata travels in HTTP headers, preserving closed JSON response shapes.

## Reproduce and interpret the limits

Use `scripts/benchmark_retrieval_history.py` with a dedicated empty scope and disposable storage. It rejects existing tombstones, preserves exact write intents for failure reconciliation and never overwrites an output directory. See [current retrieval candidates](../api/retrieval-candidates.md) for the contract and migration procedure.

The Rust regressions also cover the deterministic eight-to-four case, three history cycles, tag/type movement, review changes, stale and missing vectors, coherent snapshots, continuation, restart, interrupted projection reconstruction and encountered corruption. Existing scope tests cover tenant, project, mission, agent, private subjects and revoked credentials.

The projection retains audit data. Expiry and transitive source invalidation still require snapshot eligibility checks and can consume budget; startup reconstruction scans canonical history when required. Candidate keys add storage and write amplification proportional to tag count. This is not ANN or a token inverted index. Do not concatenate BM25 pages into a global ranking, infer absence from partial coverage, or promote hybrid retrieval from these capacity results.
