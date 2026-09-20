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
