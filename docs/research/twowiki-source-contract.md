# 2WikiMultiHopQA source contract

This source adapter prepares structural validation for a replacement multi-step
retrieval evaluation. It does not select a cohort, generate embeddings, construct
a graph or run retrieval. No candidate quality result follows from its counts.

## Research basis and source identity

The [2WikiMultiHopQA paper](https://aclanthology.org/2020.coling-main.580/)
distinguishes comparison, inference, compositional and bridge-comparison questions.
The last category combines finding connecting entities with comparison. These
categories are not interchangeable with MuSiQue decomposition-hop strata.
The [official repository](https://github.com/Alab-NII/2wikimultihop) documents
sentence-level support labels and publishes an April 7, 2021 dataset revision
with sentence-segmentation corrections. This adapter pins that archive's public
development member. It does not use the unlabeled test member or claim an official
leaderboard result.

Download the corrected archive through the official repository and retain its
exact bytes. `scripts/audit_twowiki_source.py` validates the archive and `dev.json`
SHA-256 values before parsing. It reads the named member without extracting paths.
The expected hashes are recorded in the source and
[frozen audit](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/twowiki-source-contract/benchmarks/retrieval/twowiki-source-audit-v1.json).
A replacement archive is a source-contract change, not an automatic fallback.

```bash
python3 scripts/audit_twowiki_source.py audit \
  --archive ./data_ids_april7.zip \
  --output ./twowiki-audit.json

python3 scripts/audit_twowiki_source.py verify \
  --archive ./data_ids_april7.zip \
  --output ./twowiki-audit.json
```

Creation refuses to overwrite an existing output. Verification recomputes every
field, including quarantine identities and reasons. Keep source text and generated
corpora outside the repository; the public audit contains IDs, counts and hashes.

## Structural validation and document boundary

Duplicate global query IDs fail the audit. Each record is checked for a known
category, nonempty question and answer, valid title/sentence context pairs and
resolvable sentence-level support labels. Boolean indices, negative or out-of-range
indices, missing titles, duplicate support labels and ambiguous context titles
cannot become accepted labels. Records with these defects are quarantined rather
than repaired. This validates fields consumed by the proposed retrieval adapter;
it does not certify every source annotation or semantic correctness.

A separate `document_projection` function accepts only context title/sentence
pairs. It binds document IDs to both title and sentence content, orders documents
by identity and rejects ambiguous titles. It receives no question, answer, support
label, gold entity ID or evidence triple. Any later graph builder must preserve
this boundary: source answer triples are not inferred document relations.

## Observed source capacity

| Source category | Records | Structurally eligible |
| --- | ---: | ---: |
| Bridge comparison | 2,751 | 2,674 |
| Comparison | 3,040 | 2,772 |
| Compositional | 5,236 | 5,081 |
| Inference | 1,549 | 1,517 |
| Total | 12,576 | 12,044 |

The audit quarantines 532 records: 463 have duplicate context titles and 70 have
out-of-range support indices, with one record in both categories. Even identical
repeated titles remain quarantined; the adapter does not silently select or merge
a paragraph and change the meaning of sentence references. These exclusions
change the available distribution and must accompany any subsequent cohort report.

Structural eligibility does not mean a question is disjoint from previous
MuSiQue or other evaluation history. Before running a comparison, freeze the
cross-source identity and overlap rules, category quotas, query selection,
document corpus, graph policy, sparse judgments, candidate version and budgets.
Keep any inspected relevance results in the observation history. Sentence support
labels do not exhaustively judge other documents in a merged retrieval corpus.
Public data may also appear in model training. Retrieval gains and agent-task
gains remain separate hypotheses requiring their own measurements.
