# Evaluate graph-assisted retrieval

Use this workflow to compare explicit typed paths with lexical, semantic and
hybrid retrieval on the same frozen sources. Graph construction, retrieval quality
and downstream agent performance are separate questions. The graph API is
experimental; this experiment does not admit a new default.

## External judgments and selection

The [MuSiQue repository](https://github.com/StonyBrookNLP/musique) supplies the
MuSiQue-Ans v1.0 release and its CC BY 4.0 attribution. The
[TACL paper](https://arxiv.org/abs/2108.00573), by Harsh Trivedi, Niranjan
Balasubramanian, Tushar Khot and Ashish Sabharwal, describes questions composed
from connected reasoning steps. This evaluation measures retrieval of its labeled
supporting paragraphs, not question answering or the paper's results.

The importer verifies exact train/development file hashes. It selects identifiers
by a frozen SHA-256 ordering: 30 official training queries for development and 100
official development queries reserved for comparison. Reserved strata contain
40 two-hop, 30 three-hop and 30 four-hop questions. This is a public development
subset, **not the hidden test leaderboard**. The policy and ranking profiles were
fixed before retrieval; development queries are not used to fit a new profile.
One development query warms each method, outside the measured cohort.

All paragraphs supplied with the selected questions, including distractors, form
a deduplicated union corpus of 2,311 documents. Each original supporting paragraph
has relevance grade 1; other union-corpus documents remain unjudged. Labels do not
provide exhaustive relevance coverage. Deduplication binds both title and exact
paragraph text. Questions, answers and decompositions are excluded from documents.

## Document-only relation construction

The external `document_title_graph_v1` builder accepts exactly three fields:
document ID, title and paragraph text. It rejects additional fields, including
questions, answers and support labels. The input projection and output relations
have reproducible hashes, and evaluation reconstructs them before accessing the
server.

The builder creates `same_entity` assertions for identical normalized titles and
`semantic_related` assertions when a paragraph mentions another document's full
normalized title. Normalization uses Unicode NFKC, case folding and word tokens.
Single-token titles shorter than four characters and numeric titles are excluded
from mention matching. There is no alias expansion or removal of disambiguation
suffixes. Each document selects at most four same-title neighbors and eight
mention targets, with deterministic ordering. Incoming degree can exceed these
outgoing construction limits. Parallel source observations are not independent
corroboration.

These 3,791 assertions are a deliberately simple, reproducible external heuristic.
They do not establish entity identity, causation or factual support. Provenance
records the builder version and document/title hashes. No extraction model runs
inside the database. False matches and missing links remain possible and can
explain retrieval regressions.

## Frozen comparison

The checked-in `benchmarks/retrieval/graph-multihop-protocol-v1.json` fixes:

- BM25, exact cosine, hybrid v1 and hybrid v2 baselines.
- Balanced graph paths over hybrid v2, entity-weighted paths over hybrid v2,
  balanced paths over lexical seeds, and hybrid-seeded depth-zero ablation.
- Ten final hits, identical 10,000-record/64 MiB source budgets, external E5
  vectors reused by every vector method and cosine threshold -1.
- Four anchors and the server's 100-candidate seed pool. Graph depth is two,
  with 128 nodes, 256 edges, 1,024 adjacency positions and 8 MiB affinity bytes.
- Three observations per query/method, one client, a seeded shuffle of all
  2,400 measured requests, retained RocksDB/OS caches and no result cache.

The primary declared contrast is balanced hybrid graph retrieval minus hybrid v2.
Other profiles and contrasts are exploratory, without multiplicity correction.
No ranking parameter is fitted to these reserved results. After publication, this
cohort becomes regression evidence; future tuning needs fresh evaluation data.

## Measurements and failure policy

nDCG@10 uses gain `2^grade - 1` and discount `log2(rank + 1)`. Unjudged documents
contribute zero to this judged metric. Judged Recall@10 divides returned positive
labels by all positive labels for that query. Also report the fraction retrieving
all labeled supports and the fraction retrieving none, plus each hop category.
Paired 95% percentile intervals resample queries 2,000 times. Timing repetitions
are not treated as independent relevance samples.

The runner reads every source and relation before and after the trial, compares
both history fences, verifies exact current revisions, embedding receipts, scopes,
server profiles, canonical payloads and graph proofs. It independently recalculates
path strength and score contributions. Repeated rankings and proofs must remain
identical. Graph anchors must match the independently retrieved baseline; depth
zero must preserve the hybrid v2 ordering.

Source and embedding scans must be complete. Candidate caps and bounded graph
cuts remain visible and are reported; they do not imply a global graph ranking.
A failed HTTP request, invalid proof, changed history or inconsistent repetition
fails the campaign. Requests are not retried during measurement. Partial evidence
is saved and the runner refuses to overwrite any prior report, including failures.

Server retrieval time excludes admission, serialization and transport. The graph
route returns it in `X-Qilbee-Retrieval-Micros`; older routes retain their existing
timing contracts. Client HTTP time includes serialization/transport. The reported
embedding-plus-HTTP estimate adds separately measured external query encoding
time to HTTP time; it is **not a jointly measured end-to-end request**. Model
loading, document encoding and import are separate preparation costs.

Report response bytes, canonical payload bytes, candidate/adjacency/affinity work,
cuts, and whole-server CPU/RSS observations. RSS is sampled once per second, so
short peaks may be missed. Interleaved process totals cannot attribute CPU to a
specific method. Native workstation timings are not a production SLA.

## Run in an isolated laboratory

Obtain the official release through the upstream repository. Keep downloaded
corpora, generated vectors, credentials and the database outside Git. The importer
checks the pinned files and refuses to overwrite frozen outputs:

```bash
python3 scripts/import_musique_graph.py \
  --source-dir "$RUN_DIR/data" \
  --output "$RUN_DIR/source.json" --relations "$RUN_DIR/relations.json"

python3 scripts/build_embedding_fixture.py \
  --source "$RUN_DIR/source.json" --model-dir "$E5_MODEL_DIR" \
  --output "$RUN_DIR/fixture.json" --measurements "$RUN_DIR/generation.json" \
  --overlength truncate

cargo run --release -p qilbee-server --example retrieval_lab -- \
  "$RUN_DIR/database" "$RUN_DIR/access.json" 19913
```

Set `RUN_DIR` and `E5_MODEL_DIR` to absolute private directories. Use the pinned
model artifacts described in [external embedding evaluation](external-embedding-evaluation.md).
The example binds loopback only, refuses existing data/credential paths, uses the
real platform router and production storage configuration, and issues a private
read/write credential valid for 24 hours. The credential file has mode 0600 on
Unix. It is a developer laboratory, not a production bootstrap workflow.

In another terminal, use the printed server PID for resource observation:

```bash
python3 scripts/evaluate_graph_retrieval.py prepare \
  --fixture "$RUN_DIR/fixture.json" --relations "$RUN_DIR/relations.json" \
  --credential-file "$RUN_DIR/access.json" --state "$RUN_DIR/state.json" \
  --protocol benchmarks/retrieval/graph-multihop-protocol-v1.json

python3 scripts/evaluate_graph_retrieval.py evaluate \
  --fixture "$RUN_DIR/fixture.json" --relations "$RUN_DIR/relations.json" \
  --credential-file "$RUN_DIR/access.json" --state "$RUN_DIR/state.json" \
  --protocol benchmarks/retrieval/graph-multihop-protocol-v1.json \
  --generation "$RUN_DIR/generation.json" --plan "$RUN_DIR/plan.json" \
  --report "$RUN_DIR/report.json" --server-pid "$SERVER_PID"

python3 scripts/export_graph_evaluation.py \
  --report "$RUN_DIR/report.json" --fixture "$RUN_DIR/fixture.json" \
  --output "$RUN_DIR/public-report.json"
```

Preparation writes memories, model-bound embeddings and typed assertions only in
the dedicated scope. It preserves idempotency identities across interruptions.
Evaluation performs read-only requests. On completion, stop the laboratory and
remove its access file. Retain private evidence securely for audit. A fresh import
assigns new UUIDs and may change exact-score tie order; retain the original
manifest when reproducing exact rankings.

## Interpretation boundary

The selected union corpus contains curated distractors and all labeled supports;
it is not the full MuSiQue environment or an enterprise memory stream. E5 is a
pretrained public model; absence of pretraining overlap cannot be established.
This 384-dimensional experiment cannot measure a dimensionality effect or be
pooled with the earlier 1,536-dimensional SciFact campaign.

The older exact-code, paraphrase, Portuguese and unanswerable regression fixtures
remain separate requirements. This multi-hop cohort contains answerable English
questions and does not qualify abstention, temporal resolution, consolidation or
learned-tool execution. A retrieval gain still requires a separate agent-task
comparison with the same model, prompt, tools, permissions and context budget.
