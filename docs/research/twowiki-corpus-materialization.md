# Materialize the frozen 2Wiki cohort

Materialization turns a [replayed selection](cross-source-selection.md) into a
text corpus, sparse relevance judgments and a document-only graph. It does not
generate embeddings, query the database or measure agent performance. The command
replays the entire selection against pinned sources and history before building
anything; a modified manifest cannot bypass that check.

## Documents, roles and judgments

The corpus includes every context document from each selected question, including
distractors. Identical title/sentence arrays are deduplicated across questions.
Document IDs bind the exact title and ordered source sentences with the `twowiki-`
namespace. Text is rendered as title, newline, then sentences joined by one ASCII
space. Reordering or altering sentences changes the identity.

The selected `development` role maps to the evaluator's `development` split.
The selected `confirmation` role maps to `test`. Both originate in public
2Wiki development data; `test` here is not the dataset's hidden official test set.
The source receipt preserves this mapping and the selection-manifest hash.

A document receives grade 1 for a question when it contains an original supporting
sentence. Every label records the exact document identity, zero-based sentence
index and sentence digest. Other documents in the merged corpus are unjudged;
`judgments_complete` remains false. Do not interpret sparse support labels as
exhaustive relevance judgments or report exhaustive recall from them.

## Document-only graph policy

`twowiki_document_title_graph_v1` applies the existing bounded title-mention
construction to source-qualified documents. It receives only document IDs, titles
and sentences. It accepts no question, answer, supporting label, entity ID or
gold evidence triple. Duplicate or altered document identities fail validation.

The policy uses NFKC case-folded word tokens, bounded same-title neighbors and
full-title mentions in paragraph text. It retains the prior limits of four
same-title neighbors and eight mention targets per document, with deterministic
ordering. It produces `same_entity` and `semantic_related` assertions with
construction provenance. These are lexical document-derived assertions, not
externally verified semantic truth. The policy, sentence projection, rendered
projection and relations have separate digests.

The retrieval verifier recognizes this policy explicitly, reconstructs its graph
from the frozen document projection and verifies that rendered documents match
the embedding fixture's source. Unknown or altered policies and relations fail.
The original MuSiQue policy keeps its existing identity and outputs.

## Create and verify a bundle

```bash
python3 scripts/materialize_twowiki_cohort.py materialize \
  --archive ./data_ids_april7.zip \
  --musique-source-dir ./musique-source \
  --prior-fixture ./original-source.json \
  --prior-fixture ./fresh-source.json \
  --prior-fixture ./materialized-source.json \
  --manifest ./selection.json \
  --bundle ./new-corpus

python3 scripts/materialize_twowiki_cohort.py verify \
  --archive ./data_ids_april7.zip \
  --musique-source-dir ./musique-source \
  --prior-fixture ./original-source.json \
  --prior-fixture ./fresh-source.json \
  --prior-fixture ./materialized-source.json \
  --manifest ./selection.json \
  --bundle ./new-corpus
```

Creation requires a new directory. It writes `source.json`, `relations.json`, then
`receipt.json`, flushing each file. An interrupted bundle must pass complete
reconstruction before reuse; the mere presence of a receipt is not acceptance.
Verification rejects missing, altered or symlinked bundle files. Preserve failed
bundles for diagnosis instead of overwriting them. This workflow does not claim
filesystem power-loss guarantees.

Keep source text, generated corpora and embeddings outside Git. The sanitized
receipt can be published with source/graph/selection hashes, counts, role mapping
and explicit unevaluated status. Before retrieval, separately freeze the external
encoder identity, fixture, ranking methods, budgets and evaluation protocol.

## Frozen corpus

The first selected cohort materializes 1,148 unique documents and 328 relations,
with 40 development and 120 reserved-confirmation queries. The
[corpus receipt](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/twowiki-materialization/benchmarks/retrieval/twowiki-corpus-receipt-v1.json)
binds the exact source and graph. Relation counts describe this construction;
they are not evidence of graph coverage, relevance gains or better reasoning.
