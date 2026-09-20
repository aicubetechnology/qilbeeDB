# Evaluate retrieval with external SciFact judgments

Use this workflow to compare QilbeeDB's lexical, cosine and experimental hybrid
retrieval on a public, externally judged collection. Keep this study separate from
the fictional agent-memory diagnostic corpus. A scientific claim retrieval score
does not establish truth verification, clinical correctness or better agent tasks.

## Dataset and split contract

The importer pins the [BEIR SciFact distribution](https://github.com/beir-cellar/beir)
by SHA-256 and records hashes of every imported source file. It uses all 5,183
abstracts. Source text is the nonempty title and abstract joined by one newline.
Document IDs retain the original strings; QilbeeDB-generated memory UUIDs and
revisions are retained separately in the persistent evaluation manifest.

The official BEIR test set has 300 queries and is preserved. Of 809 training
queries, two have case-insensitively identical text to test queries. The importer
excludes only those development duplicates, records their IDs, and uses the other
807 for development. It does not resample the test set or rewrite the relevance
judgments. This is the BEIR retrieval split, not a claim to reproduce the original
SciFact verification leaderboard's private test labels.

The [original SciFact data](https://github.com/allenai/scifact) was introduced by
Wadden and colleagues in *Fact or Fiction: Verifying Scientific Claims* (2020).
Its [license file](https://github.com/allenai/scifact/blob/master/LICENSE.md)
identifies CC BY 4.0 for claims/evidence annotations and ODC-By 1.0 for the S2ORC
abstracts. Retain upstream attribution and license information with downloaded or
derived data. This repository publishes the importer, provenance and evaluation
results; download the corpus from the identified upstream source.

Qrels have binary grade 1 for relevant evidence. Unlisted documents are unjudged,
not proven irrelevant. `judgments_complete` is false: report **judged Recall@10**,
leave exhaustive recall unavailable, and describe nDCG as qrel-based. An abstract
can be relevant because it supports or contradicts a claim; the retrieval metric
does not judge whether an agent interpreted that evidence correctly.

## Keep model configurations separate

The E5 example below is an optional, separate 384-dimensional experiment. It does
not replace a deployment using OpenAI `text-embedding-3-small` with 1536 dimensions,
or a separately declared 3072-dimensional model space. Reuse frozen embeddings
from the selected provider when available; do not regenerate them just to rerun
retrieval. A captured vector-set digest identifies the frozen representation, not
an immutable provider model revision.

Give each campaign a dedicated authorized mission and private subject. Tag filters
restrict the ranking corpus, but source scan budgets also count nonmatching and
deleted records in that namespace. Do not share an evaluation namespace between
large campaigns. Preserve a separate state manifest and unique run identity when
recreating a cleaned campaign; do not reuse terminal cleanup receipts.

A 50-query development subset selected by identifier and the full 807-query E5
development set are different protocols. Record their selection before inspecting
rankings and keep their results separate. Both may preserve all 300 official test
queries; never pool their means or infer a dimensionality effect across different
models and development sets.

## Freeze the external representation

Download the archive from:

```text
https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/scifact.zip
SHA-256: 536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165
```

```bash
python3 scripts/import_scifact.py \
  --archive /secure/path/scifact.zip --output /secure/path/scifact-text.json
/secure/path/embedding-venv/bin/python scripts/build_embedding_fixture.py \
  --source /secure/path/scifact-text.json \
  --model-dir /secure/path/e5-artifacts --overlength truncate \
  --output /secure/path/scifact-fixture.json \
  --measurements /secure/path/scifact-generation.json
```

Use the optional dependencies and pinned model files described in
[external embedding evaluation](external-embedding-evaluation.md). Preserve full
abstracts for lexical retrieval and explicitly record right truncation to 512
tokens for the encoder. The policy is part of the model-space revision. Keep the
frozen vectors and per-input token/truncation measurements; do not silently swap
an encoder implementation between baseline and hybrid requests. Public pretrained
model exposure to related data is uncontrolled and must be disclosed.

## Select a candidate using development queries only

The declared grid in `benchmarks/retrieval/rrf-development-grid.json` explores
20 combinations: rank constants 5, 10, 20 and 60; lexical weights 0.2, 0.35, 0.5,
0.65 and 0.8. Semantic weight is one minus lexical weight. Every combination uses
at most 100 candidates per channel and returns ten results. Ranks start at one;
missing channels contribute zero; ties use source UUID ascending.

The [analysis of hybrid fusion functions](https://arxiv.org/abs/2210.11934)
finds that RRF can be sensitive to its parameters. This motivates measuring a
bounded grid on development data; it does not imply that a particular setting or
RRF itself is universally superior. The current experiment holds the encoder,
corpus, tokenization, retrieval components and candidate budget fixed.

```bash
python3 scripts/tune_retrieval.py \
  --fixture /secure/path/scifact-fixture.json \
  --credential-file /secure/path/scifact-credential.json \
  --scope-file /secure/path/scifact-scope.json \
  --state /secure/path/scifact-state.json \
  --cache /secure/path/scifact-development-candidates.json \
  --grid benchmarks/retrieval/rrf-development-grid.json \
  --report /secure/path/scifact-development-selection.json
```

The utility retrieves only development queries, checks scope/revision/model
coverage, and saves complete top-100 lexical and dense ranks and raw scores. The
cache is bound to the fixture and source manifest. Reusing it requires unchanged
sources. No reserved query is executed by this utility. It selects highest mean
development nDCG, then judged recall, then the closest lexical weight to v1, then
the closest rank constant. Selection scores are optimistic development evidence,
not held-out confidence estimates. No profile is automatically installed in the
server; a chosen configuration needs an immutable version, implementation tests
and a feature PR before clients can request it.

## Evaluate the frozen choice

Pin separate [trial plans](retrieval-evaluation.md) for each implemented ranking
version before executing the 300 test queries. Preserve the old version as a
baseline. Use the same source manifest and query vectors throughout. Check complete
source coverage independently from the 100-candidate channel cap. Report both
counts, every losing query and paired uncertainty; do not pool results with the
small fictional corpus or tune again on the observed test scores.

For actual generation-to-response latency, run the optional client-cycle utility:

```bash
/secure/path/embedding-venv/bin/python scripts/measure_retrieval_cycle.py \
  --fixture /secure/path/scifact-fixture.json \
  --credential-file /secure/path/scifact-credential.json \
  --state /secure/path/scifact-state.json \
  --model-dir /secure/path/e5-artifacts --overlength truncate \
  --split test --ranking-version weighted_rrf_v1 --repetitions 1 \
  --report /secure/path/scifact-client-cycle.json
```

This measures each cycle directly, including external generation and HTTP. Every
regenerated vector must exactly match its frozen value. Session loading and source
verification are excluded. Lexical requests require no generation. Model generation
cost is not imputed as zero: local hardware and energy costs remain unmeasured.
Use an otherwise idle trial server; one serial warm client is not a concurrent
load test or a service-level guarantee. Better retrieval still requires a separate
fixed-model agent-task study before claiming better autonomous behavior.

## Complete scans with larger external vectors

For the full SciFact corpus with 1536-dimensional frozen vectors, the 0.5.0
64 MiB ceiling can stop the hybrid source scan before all 5183 documents are
examined. Configure an adequate operator scan ceiling and pin the same explicit
request budget in the trial plan. See [retrieval capacity](../operations/retrieval-capacity.md).
For example, add `--scan-bytes-limit 134217728` when writing a plan or collecting
development candidates, after the server has been configured to allow it.
The budget is recorded in the plan and report; it cannot be overridden during
execution of a pinned plan. Check full source and embedding coverage before any
relevance comparison. Do not reduce vector dimensions or merge independent BM25
pages to bypass an incomplete scan.

## Compare additional development fusion candidates

If the first development grid is insufficient, declare an expanded search before
running it and retain the original result. The committed expanded grid has 114 RRF
combinations: six rank constants and 19 lexical weights. It contains the original
20 combinations. `compare_development_fusion.py` additionally evaluates 19 convex
combinations using per-channel min-max normalization over the scoped top-100
candidates. A constant nonempty score range maps to one; an absent channel
contributes zero. This is a declared alternative, not a raw addition of BM25 and
cosine scores. The total expanded search has **133 distinct candidates**.

```bash
python3 scripts/compare_development_fusion.py \
  --fixture /secure/path/scifact-fixture.json \
  --state /secure/path/scifact-state.json \
  --cache /secure/path/development-candidates.json \
  --grid benchmarks/retrieval/rrf-expanded-development-grid.json \
  --report /secure/path/development-fusion-comparison.json
```

This offline comparison verifies fixture and source-manifest hashes, refuses test
queries and requires complete development query coverage. It reuses previously
validated server channel candidates and makes no embedding-provider calls. Its
normalization refers to those candidate lists, not to the entire corpus or to a
calibrated probability. Expanding the search increases selection bias; development
scores must not be presented as held-out gains.

The resulting server profile `weighted_rrf_v2` fixes lexical weight **0.25**, semantic
weight **0.75** and rank constant **2**. It retains the same tokenization, exact
cosine, 100-candidate channel cap and UUID tie-break as v1. Both profiles remain
experimental; v1 keeps its original 0.5/0.5 weights and constant 60.

## Interleave both profiles with shared baselines

For a full multi-profile comparison, the following evaluator freezes the fixture,
source manifest, profiles, budgets and concurrency. It sends one measured request
per query and method, with a seeded shuffle across all methods. A single development
query warms each method before measurement. Lexical and cosine results are shared
baselines for both hybrid versions. All results come from the server's HTTP API;
no offline fusion replaces a hybrid response in this comparison.

```bash
python3 scripts/evaluate_retrieval_profiles.py \
  --fixture /secure/path/scifact-fixture.json \
  --state /secure/path/scifact-state.json \
  --plan /secure/path/interleaved-plan.json --write-plan
python3 scripts/evaluate_retrieval_profiles.py \
  --fixture /secure/path/scifact-fixture.json \
  --state /secure/path/scifact-state.json \
  --plan /secure/path/interleaved-plan.json \
  --credential-file /secure/path/evaluator.json \
  --report /secure/path/interleaved-report.json \
  --container qilbeedb-local
```

Defaults are two concurrent HTTP requests and a 128 MiB lexical/hybrid scan budget.
The server must admit these settings. Specify matching `--concurrency`,
`--scan-bytes-limit` and `--seed` values at plan creation and execution when using
other conditions. Use the same frozen manifest; this command does not prepare or
modify source records. It refuses an existing final report and verifies sources
before and after measurement.

Every response must have full corpus coverage, the expected scope and profile,
current source revisions, exact embedding receipts and no duplicate results.
Failures make the comparison invalid. The report includes all rankings, raw scores,
contributions, candidate counts, bytes, latency, pairwise wins/losses and exploratory
paired bootstrap intervals. Container CPU/memory counters cover the combined
interleaved campaign; they cannot attribute resource use to an individual method.
There is one observation per query/method, so no repeated-query stability estimate.
Generation latency and cost remain unmeasured in this run, not zero.

## Research informing subsequent experiments

The current release implements fixed, versioned fusion. Recent research motivates
additional hypotheses; the following mechanisms are **not implemented or validated
by this release**:

| Research | Finding or proposed mechanism | QilbeeDB experiment to design |
| --- | --- | --- |
| [Know When to Fuse, COLING 2025](https://aclanthology.org/2025.coling-main.290/) | Fusion gains depend on domain adaptation and weight selection; the best individual retriever remains a necessary baseline | Reserve multilingual and domain-shift test sets; include losses against the stronger individual channel |
| [QuDAR, ACL 2026](https://aclanthology.org/2026.acl-long.1791/) | Uses query-dependent margins and LLM relevance signals to adapt retrieval and expansion weights | Compare a separately versioned adaptive rule against frozen static profiles, with scope-local features, explicit cost and independent test data |
| [LLM-Independent Adaptive RAG, EMNLP 2025](https://aclanthology.org/2025.emnlp-main.439/) | Studies external features for deciding when to retrieve without LLM-based uncertainty estimation | Evaluate retrieval necessity and evidence sufficiency independently; returning a nearest neighbor is not proof of useful evidence |

These are research directions, not a roadmap commitment or evidence that QilbeeDB
reproduces the papers' reported gains. Query expansion, learned rerankers and model
calls would require explicit external execution contracts; no provider credentials
or model inference are introduced into the database by the present experiments.

## Recorded 0.6.0 results

The [complete real-vector report](scifact-results.md) covers all 300 SciFact test
queries, individual regressions, uncertainty, measured capacity and the separate
40-source diagnostic. The [experience-memory design](experience-memory-design.md)
explains the additional evidence needed to evaluate downstream agent learning.
