# Cross-source retrieval evaluation capacity

Changing datasets does not by itself create unobserved evaluation questions.
This audit compares structurally valid 2WikiMultiHopQA records against declared
MuSiQue observation history before cohort selection. It runs no retrieval and
accepts no query quota or seed. Its output is an upper bound on selection capacity;
mutual exclusions between future selected questions can reduce that capacity.

## Versioned identity projection

`musique_twowiki_overlap_projection_v1` uses normalization version
`nfkc_casefold_unicode_words_v1`: NFKC case folding, Unicode word tokens joined
by one space, and the normalized literal for punctuation-only strings. Source
archives, source members, aliases and historical fixture bytes are hash-bound.

| Identity | Comparison |
| --- | --- |
| Query ID | Dataset-qualified; numeric component IDs from unrelated sources are never equated |
| Question | Complete questions and official MuSiQue subquestions, normalized without generated decompositions |
| Answer | Final answers, declared aliases and explicitly annotated MuSiQue intermediate answers |
| Annotated entity | 2Wiki evidence endpoints and their available aliases/demonyms; never presumed to be intermediate answers |
| Support title | Normalized title, independently of paragraph segmentation |
| Support text | Digest of normalized paragraph text, independently of title |

Candidate answers and annotated entities compare against the union of historical
answers and annotated entities. The report preserves the candidate's annotation
role: a matching evidence endpoint is an `annotated_entity` conflict. One entity
can also be the final answer, so categories can overlap. Their counts must not be
summed into unique excluded questions. Gold annotations serve exclusion only;
they must not become graph edges, query expansion or runtime retrieval input.

Title-level exclusion is stronger than exact paragraph equality. It deliberately
excludes a matching supporting title even if the datasets divide its text
differently, changing the available distribution. Semantic equivalence beyond
these explicit identities is not established.

## Alias and structural coverage

The [source audit](twowiki-source-contract.md) first quarantines ambiguous context
titles and invalid sentence labels. Projection additionally checks evidence and
entity-identity shape. The official `id_aliases.json` member is JSON Lines; both
aliases and demonyms are normalized. A referenced entity missing from that member
is quarantined before conflict counting. Absent source entity IDs remain explicit
unknown coverage; neither an empty ID list nor a successful lookup proves all
possible aliases are known. Malformed evidence fails instead of silently skipping
an exclusion. None of these checks proves semantic correctness of the annotations.

## Create and replay an audit

Retain the pinned archives and all three previously observed fixture files. Supply
additional history only when it matches the supported source identities; this
command cannot discover unlisted observations or import arbitrary external runs.

```bash
python3 scripts/audit_cross_source_overlap.py audit \
  --archive ./data_ids_april7.zip \
  --musique-source-dir ./musique-source \
  --prior-fixture ./original-source.json \
  --prior-fixture ./fresh-source.json \
  --prior-fixture ./materialized-source.json \
  --output ./cross-source-capacity.json

python3 scripts/audit_cross_source_overlap.py verify \
  --archive ./data_ids_april7.zip \
  --musique-source-dir ./musique-source \
  --prior-fixture ./original-source.json \
  --prior-fixture ./fresh-source.json \
  --prior-fixture ./materialized-source.json \
  --output ./cross-source-capacity.json
```

Creation refuses an existing output. Verification recomputes all fields from the
sources and declared history, including the normalized history-projection digest,
quarantine IDs, per-category counts and coverage. Altered metadata or counts fail.
The public report contains counts, hashes and limited quarantine identities rather
than question, answer or paragraph text.

## Interpretation

An audit is not a selected cohort or evidence of better ranking. Before measuring,
freeze category quotas, the development/confirmation roles, selection order and
seed, mutual exclusions, candidate identity, budgets and relevance judgments.
2Wiki categories retain their own meanings; they are not MuSiQue hop strata.
Public-source identity exclusions do not establish absence from model training,
statistical independence or agent-task improvement. A sparse support judgment set
also cannot establish exhaustive relevance in a merged retrieval corpus.

## Recorded capacity against 470 observed questions

The [frozen audit](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/cross-source-overlap/benchmarks/retrieval/twowiki-cross-source-capacity-v1.json)
contains 12,576 source records, 532 structural quarantines and one additional
known-missing-alias quarantine. After exclusions against the declared 470-question
history, 9,217 records remain eligible against that history alone.

| Category | Eligible against history |
| --- | ---: |
| Bridge comparison | 2,296 |
| Comparison | 1,665 |
| Compositional | 3,765 |
| Inference | 1,491 |

Among 12,044 structurally valid projected records, 1,799 lack answer entity IDs
and 4,331 have no evidence-identity triples. These annotation gaps remain in the
report; no complete alias-coverage claim follows from these eligibility counts.

The [cohort selector](cross-source-selection.md) applies the declared exclusions
within and between development and confirmation roles, with explicit ordering,
quotas and full-manifest replay. Capacity alone does not guarantee its success.
