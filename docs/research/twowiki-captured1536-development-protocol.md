# Captured 1536-dimensional development protocol

**Status: preparation only.** This protocol binds a new development comparison to
an externally captured vector set. It does not contain measured retrieval results
or qualify any ranking method as a production default.

The frozen protocol is
`benchmarks/retrieval/twowiki-captured1536-development-v1.json`.
It uses the same source documents, document-only graph, cohort selection, 40
development questions and 120 reserved questions as the earlier 2Wiki comparison.
Only the external vector fixture, declared embedding identity and protocol
identity change. The previous 384-dimensional results remain a separate study.

## Inputs and identity

The fixture contains 1,148 documents and 160 externally generated query vectors.
Its space is `openai / text-embedding-3-small / captured-6c498918389b3b10e8a0dcb7`
with 1,536 dimensions. The revision identifies the input/vector manifest, not an
immutable provider weight revision. Newly generated vectors must not reuse this
snapshot identity. The complete fixture and manifest hashes are pinned in the
protocol; exact corpus fields, query roles, graph identity and selection provenance
are independently checked before preparation or evaluation.

Follow the [captured-vector handoff and readback procedure](captured-vector-handoff.md)
before admitting captured inputs. Keep the original capture manifest immutable
and associate separate verification receipts with it. Provider generation remains
external to the database.

## Comparison

The seven methods remain lexical BM25, semantic cosine, weighted RRF V1, weighted
RRF V2, balanced graph retrieval, strength graph retrieval and a depth-zero graph
control. Graph seeds use weighted RRF V1. The primary comparison remains strength
versus weighted RRF V1. Ranking formulas, work budgets, result count, three
repetitions and single-client execution are unchanged.

The evaluator and exporter reject substitutions of fixture, embedding space,
source, graph, roles, protocol settings or snapshot provenance. A different
protocol version cannot silently select the captured fixture. No reserved query
is measured by this development protocol.

## Measurement boundary

The capture provides batch-level generation evidence. It does not provide a
separable generation duration for each query. Do not divide batch times among
inputs, fill missing values with zero, or interpret retrieval-only latency as
complete generation-to-response latency. The evaluator's report representation
for absent per-query generation measurements must be validated before executing
this comparison. No timing or relevance conclusion is established by input
qualification alone.

Report sparse judgment coverage, per-category results, candidate and graph
truncation, response sizes, uncertainty and losses as well as average relevance.
A development benefit would still require a separately frozen reserved comparison
and downstream agent-task evaluation before broader conclusions.

The evaluator now validates its existing numeric per-query timing inputs before
any HTTP request. Missing measured-query entries, mismatched fixture identity,
non-finite or negative durations and batch-only timing input fail immediately.
Reserved-query timings are not required for a development run. This preflight
does not invent unavailable measurements or implement the pending representation
for batch captures; the latter remains a separate contract requirement.
