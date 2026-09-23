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
