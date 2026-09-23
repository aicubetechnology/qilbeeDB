# ARM64 integrity verification cost

This development experiment compares software SHA-256 with the existing
RustCrypto runtime-selected ARM64 implementation. It preserves SHA-256 outputs,
stored receipts, candidate selection and ranking. No vector cache is introduced.
The candidate reduced retrieval cost in this environment; this is not a claim of
better relevance, agent reasoning, or performance on every deployment.

## Method

The comparison used the [captured 1,536-dimensional corpus](twowiki-captured1536-development-results.md):
1,148 public documents with externally supplied embeddings. Two isolated real
server containers read separate copies of the same persisted database, including
record UUIDs and receipts. Reimporting was avoided because UUID tie breaks can
change rankings. Each container had two CPUs and a 2 GiB memory limit on the same
ARM64 host. The Linux environment reported SHA-256 instruction support.

The candidate runtime source was `d079b41765bf53b44fb5c5fa49897df1a48a7535`;
the baseline runtime source was `985e1749dea6635f01c0313a0c055c5ac288a7b8`.
The relevant change enables `sha2`'s `asm` feature only for `aarch64`, retaining
version 0.10.9 and locking its assembly dependency. The dependency selects its
hardware path at runtime and retains software fallback. Target CPU instructions
were not forced. The x86 feature configuration is unchanged.

For repeated reads, 40 development queries ran through cosine, weighted RRF v1
and graph strength, twice per method and implementation: 240 pairs and 480 HTTP
requests. Each implementation first received one warmup request per method. The
measured requests alternated baseline/candidate order. Concurrency was one.

A separate first-read experiment used the first six development query IDs in
sorted order, cosine retrieval and two repetitions: 12 pairs and 24 requests.
The corresponding server process restarted before every measured request.
**Process-cold does not mean disk-cold**: operating-system and device caches were
not cleared. Reserved queries remained unopened. All embeddings were reused;
provider generation latency and cost are outside this comparison.

## Observations

All compared responses matched after removing timing and the graph evaluation
clock. This comparison included ordering, scores, current record revisions,
source content and relation evidence. Change-feed fences stayed unchanged.

| Repeated-read method | Baseline p50 / p95 (ms) | Candidate p50 / p95 (ms) | Mean paired change (ms), exploratory 95% interval |
| --- | ---: | ---: | ---: |
| Cosine | 201.64 / 293.74 | 136.00 / 197.52 | -67.98 [-71.28, -64.99] |
| Weighted RRF v1 | 206.08 / 240.84 | 142.87 / 167.68 | -68.40 [-78.02, -61.19] |
| Graph strength | 206.37 / 249.37 | 142.70 / 161.06 | -66.64 [-71.36, -63.05] |

For process-cold cosine reads, p50/p95 changed from 241.74/289.84 ms to
189.30/224.36 ms. The mean paired change was -58.61 ms, with an exploratory
interval of [-65.54, -52.32] ms. This smaller cohort does not establish a reliable
production tail-latency bound.

Intervals average the two paired repetitions within each query before taking a
paired query bootstrap: 2,000 draws, seed 20260923, percentile 95%, without
multiplicity correction. They describe this experiment's query variation, not
uncertainty across machines, deployments or future workloads.

During the repeated-read window, process CPU time was 51.90 seconds for the
baseline and 35.61 seconds for the candidate. Observed peak RSS was 68,161,536 and
68,362,240 bytes respectively. These are whole-process observations, not
per-method attribution or evidence of reduced memory usage. Restarted-process
CPU counters are not combined into a first-read resource claim.

The [machine-readable observations](https://github.com/aicubetechnology/qilbeeDB/blob/eb17e071f394d941229c5006b6db7d2a64646793/benchmarks/retrieval/arm64-sha256-cost-report.json)
retain individual latency pairs, query identifiers and aggregate calculations.

## Reproduction and limits

Use the frozen corpus and the same imported record identities for both builds.
Keep scope, embedding space, ranking versions, budgets, result limits, hardware
limits and concurrency identical. Confirm current records and relation evidence
before comparing responses. Alternate execution order, report cold and repeated
reads separately, and record whether filesystem caches were cleared. Do not
replace unavailable embedding-generation measurements with zero.

Only one ARM64 environment was measured. Concurrent workloads, larger corpora,
other dimensions, x86 runtime behavior and ARM64 hardware without SHA instructions
were not qualified by this comparison. Runtime software fallback was reviewed in
the dependency source, but was not exercised on a CPU lacking those instructions.
This evidence admits no new ranking default and does not measure agent ability.
