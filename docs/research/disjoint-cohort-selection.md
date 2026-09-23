# Reproducible overlap-controlled cohort selection

A fresh query ID does not necessarily identify an independent evaluation case.
The [MuSiQue paper](https://aclanthology.org/2022.tacl-1.31/) describes controlling
split overlap through component questions, their answers and associated support
paragraphs. QilbeeDB's selector applies an explicit, replayable exclusion policy
before retrieval. This is an evaluation tool, not a database ranking policy.

## Selection contract

`scripts/select_disjoint_musique.py` checks the exact official train and public
development source hashes and every supplied prior fixture. Prior question IDs
and normalized texts must match those sources. Supply every previously used
fixture: the tool cannot discover omitted evaluation history.

The policy excludes shared full-question text, component IDs, component-question
text, intermediate/final answers, final-answer aliases and supporting paragraphs.
It checks these identities against prior questions and against already selected
questions. Text uses NFKC case folding and joined Unicode word tokens;
punctuation-only values retain their normalized literal. Supporting paragraphs
bind normalized title and text. Empty values and invalid support references fail.

Answerability and answer/support annotations are used for eligibility and overlap
control. They are not retrieval results and are not used to create graph edges.
Do not describe this procedure as annotation-free selection. Retrieval metrics,
rankings and model outputs do not enter the selection process.

The selector fills four-, three-, then two-hop quotas in that fixed order. Each
stratum is ordered by SHA-256 of the declared seed, a newline and the query ID,
with query ID as the tie-break. It rejects a row that overlaps a previous choice.
Insufficient capacity fails without relaxing quotas or writing a partial manifest.
An existing manifest is never overwritten. This greedy policy changes the source
distribution and is not uniform sampling or a proof of statistical independence.

```bash
python3 scripts/select_disjoint_musique.py select \
  --source-dir ./musique-source \
  --prior-fixture ./previous-source.json \
  --prior-fixture ./another-previous-source.json \
  --source-split train \
  --seed qilbeedb-disjoint-supports-v1 \
  --quotas 80 40 20 \
  --manifest ./new-selection.json

python3 scripts/select_disjoint_musique.py verify \
  --source-dir ./musique-source \
  --prior-fixture ./previous-source.json \
  --prior-fixture ./another-previous-source.json \
  --manifest ./new-selection.json
```

Verification recomputes the complete selection and compares all manifest fields,
including hashes, exclusions, quotas, ordering, IDs and provenance. It does not
merely check that submitted IDs exist. Preserve the exact source and prior-fixture
bytes needed for replay. The manifest contains IDs and digests rather than source
paragraphs or credentials.

## Frozen selection and availability

The [frozen manifest](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/disjoint-cohort-selection/benchmarks/retrieval/musique-support-disjoint-selection-v1.json)
selects 140 public-training questions: 80 two-hop, 40 three-hop and 20 four-hop.
It excludes 330 unique questions from the two declared historical fixtures.
The exact selector and an independent overlap projection both verified zero
violations of the declared overlap policy. No retrieval quality has been measured
on this selection, and no embeddings were generated during selection.

After historical exclusions, the public development pool could not satisfy a
four-hop quota under this policy. The selected public-training subset is new to
this evaluation history; it is **not MuSiQue's official held-out test split** and
is not guaranteed absent from embedding or language-model training. Shared
non-supporting distractors and semantically equivalent answers can remain.
Report these limits rather than calling the cohort fully independent.

This tool selects and verifies IDs. Materializing the corpus, generating external
embeddings, binding the chosen candidate to a frozen run and executing a comparison
are subsequent steps. Existing observed cohorts remain regression data. Do not
use the newly selected queries for tuning and then call them confirmation data.

## Materialize and verify a corpus bundle

`scripts/materialize_disjoint_musique.py` replays the complete selection before
materializing its documents and sparse support judgments. Choose an existing
historical fixture for warm-up. Its exact file digest must be part of the frozen
exclusion history, and its development queries must be unique official-training
questions. Reserved queries cannot become warm-up queries.

```bash
python3 scripts/materialize_disjoint_musique.py materialize \
  --source-dir ./musique-source \
  --prior-fixture ./previous-source.json \
  --prior-fixture ./another-previous-source.json \
  --warmup-fixture ./previous-source.json \
  --manifest ./new-selection.json \
  --bundle ./new-corpus

python3 scripts/materialize_disjoint_musique.py verify \
  --source-dir ./musique-source \
  --prior-fixture ./previous-source.json \
  --prior-fixture ./another-previous-source.json \
  --warmup-fixture ./previous-source.json \
  --manifest ./new-selection.json \
  --bundle ./new-corpus
```

The exclusive output directory contains `source.json`, `relations.json` and a
`receipt.json` written last. Verification reconstructs both artifacts from the
pinned official sources and checks their complete contents and receipt. The
receipt binds the source, graph, selection and warm-up fixture digests, along
with counts and split provenance. A file's existence alone is not completion.
A missing, truncated, modified or mismatched artifact fails verification.

After an interruption, retain the incomplete directory for diagnosis and use a
new output path for a fresh attempt. The tool does not overwrite or silently
repair existing bundles. Verify the bundle before passing its source file to
an external embedding generator or downstream evaluation runner.

Document-only graph construction receives exactly document IDs, titles and
paragraph text. Questions and support labels define the evaluation judgments,
not graph edges. The graph's source digest is recalculated after recording the
actual split mapping and replay evidence; provenance changes cannot leave the
graph bound to an earlier source description.

For the frozen 140-query selection, `development` contains the 30 previously
observed training questions for warm-up only, and `test` contains the 140 queries
reserved for this evaluation. This internal split label does not convert public
training data into the official benchmark test set. Materialization performs no
embedding generation or retrieval and does not establish relevance improvements.

The [verified materialization receipt](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/disjoint-corpus-materialization/benchmarks/retrieval/musique-support-disjoint-corpus-receipt-v1.json)
binds 3,175 deduplicated documents and 6,257 document-derived relations for this
170-query bundle. A separate reconstruction from official source rows confirmed
the exact query texts, split assignments, support judgments and document union.
These are corpus-integrity results, not retrieval or agent-quality measurements.
