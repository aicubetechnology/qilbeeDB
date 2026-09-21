# Graph memory: evidence and implementation boundaries

Reviewed September 21, 2026. This document extends the
[experience memory research map](experience-memory-design.md) with the supplied
graph research. It distinguishes source findings, QilbeeDB engineering proposals
and implemented contracts. No experiment from these sources has been reproduced
in QilbeeDB, and no graph-based quality improvement is claimed.

## Evidence reviewed

| Source | Evidence and limitation | Engineering question |
| --- | --- | --- |
| [Graph-based Agent Memory, v1](https://arxiv.org/html/2602.05665v1) | Survey organized around extraction, storage, retrieval and evolution. A taxonomy is not a measured improvement for this database. | Can each lifecycle stage expose provenance, authorization, invalidation and an independently testable contract? |
| [MAGMA, v2](https://arxiv.org/html/2601.03236v2) | Multi-graph memory separates semantic, temporal, causal and entity relations with adaptive traversal. Its LongMemEval average improves, but some question categories trail its baselines. The authors identify extraction errors, propagation of incorrect links and extra engineering cost as limitations. | Do typed relations and bounded traversal improve held-out tasks enough to justify construction and retrieval cost? |
| [Graph-Structured Persistent Memory for Efficient LLM-Based Computer Use Agents](https://www.mdpi.com/2075-1680/15/6/415) | Publisher-indexed excerpts describe state/action graphs and reusable tools; direct full-text retrieval failed in this review. Methods and results have not been fully assessed. | How should replay verify the current environment and tool version before reusing an earlier action? |
| [Mem0's State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026) | First-party vendor discussion and reported benchmark results. Its claims are not a common controlled QilbeeDB comparison. | Which source weighting and retrieval mechanisms survive evaluation under the same model, corpus and resource budget? |

The supplied [alphaXiv link](https://www.alphaxiv.org/abs/2602.05665) is a mirror
of the same survey, not independent evidence. The
[Neural Maze tutorial](https://theneuralmaze.substack.com/p/building-agent-memory-with-knowledge)
is implementation context rather than a scientific comparison. Its visible
introduction was reviewed, but the complete downloadable implementation was not
evaluated. Decisions about measured benefits must rely on primary research and
reproducible local experiments.

The earlier supplied ReasoningBank, AI Meets Brain, ZenBrain, Dream-RSI,
Perplexity and SSRN sources remain recorded, including retrieval limitations, in
the [existing evidence map](experience-memory-design.md). Product descriptions,
secondary articles and inaccessible methods do not establish validated features.

## Current platform boundary

The current [derived memory contract](../api/derived-memory.md) stores explicit
dependencies on exact source revisions and checks transitive eligibility.
It is a graph of declared evidence dependencies. It does not extract entities,
discover causal relationships or expand hybrid search through semantic graph
neighbors. The 0.12.0 [evidence graph API](../api/memory-evidence-graph.md)
now exposes that ancestry in one scoped snapshot with exact revisions, bounded
traversal and native company administration. It does not change the search algorithm.

The 0.12.0 [durable graph lifecycle](../architecture/durable-graph-lifecycle.md)
corrects three demonstrated defects in the separate Rust graph engine: IDs reused
after restart, relationship IDs reused after restart, and old records exposed by
recreating a name. Durable generations and counters are prerequisites for stable
references. They are not an integration of that engine with HTTP memory retrieval.

External workers continue to generate embeddings and perform model inference.
The database should preserve claims, versions, evidence and admission decisions.
An agent-generated causal assertion must not silently become an observed cause,
and repeated copies of a source must not count as independent corroboration.
These are QilbeeDB contract requirements inferred from the failure risks; they
are not guarantees provided by a graph representation alone.

The 0.13.0 [typed relation ledger](../api/typed-memory-relations.md)
implements the first part of this contract. Its six kinds separate semantic,
entity and temporal assertions from causal claims, support and contradiction.
This is an engineering adaptation informed by MAGMA's distinction between graph
views; it is not a reproduction of MAGMA's extraction, adaptive traversal or
results. Assertions remain externally supplied claims. The
[typed traversal API](../api/typed-memory-graph.md) now adds bounded incoming and
outgoing discovery with current endpoint checks, explicit cuts and native company
administration. The separate [relation feed](../api/typed-relation-changes.md)
adds history-bound delivery, durable subject-owned progress, diagnostics and
explicit reconciliation. The [graph-assisted retrieval API](../api/graph-assisted-retrieval.md) now adds
experimental server-owned path ranking with reproducible anchors, current source
checks and explicit work limits. The separate [MuSiQue comparison](graph-retrieval-results.md)
reports gains and regressions under frozen conditions; it does not establish an
agent-task benefit or admit a default ranking change.

## Context-bound consolidation prerequisites

MAGMA section 3.4 describes asynchronous inference over a local neighborhood.
[ReasoningBank](https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/)
describes retrieval, extraction and consolidation of experience; its initial
consolidation uses direct addition, with more advanced strategies left as future
work. Neither description establishes the storage validity contract below.

**Engineering inference:** a relation inferred from several memories must depend
on all declared supporting context, not just the two graph endpoints. The
unreleased [relation evidence extension](../api/typed-memory-relations.md#bind-the-context-used-for-inference)
adds exact same-scope context references, bounded transitive checks, stale-source
publication rejection and invalidation at traversal and retrieval. Immutable
history survives invalidation. This is a prerequisite for an external consolidator;
it does not implement the worker, automatically discover missing evidence, or
reproduce a published quality gain. Durable job leases, fenced batch publication,
unknown-outcome accounting and comparative evaluation remain required.

## Implementation and acceptance map

| Capability | Current status | Required observable contract and evidence |
| --- | --- | --- |
| Stable general graph identity | Implemented in the 0.12.0 lifecycle feature | Restart preserves allocation; a retired name creates a fresh generation; stale handles cannot publish writes; failed preparation publishes nothing |
| Revision-bound evidence dependencies | Available through derived memories | Existing source eligibility checks apply to reads and retrieval; changes, rejection and expiration suppress stale derived context |
| Typed memory relations | Implemented in the 0.13.0 [relation ledger](../api/typed-memory-relations.md) | Exact eligible endpoint revisions, typed directed assertions, authenticated reporter, declared origin, extraction/model identity, validity, review and reversible retirement; atomic indexes, immutable history and crash-recoverable receipts |
| Evidence ancestry traversal | Implemented in the 0.12.0 evidence graph API | Exact-scope outgoing `derived_from` relations, one snapshot and clock, eligible payloads, depth/node cuts, shared admission and company workspace access |
| Typed memory neighborhood traversal | Implemented in the 0.13.0 typed graph API | Exact scope before incoming/outgoing discovery, canonical endpoint revalidation, direction and kind selection, bounded nodes/edges/bytes/work, explicit cuts, adjacency completeness checks and native company audit; standalone entity extraction/resolution remains unimplemented |
| Relation change consumption | Implemented in the 0.13.0 [relation feed](../api/typed-relation-changes.md) | Atomic lifecycle events, history-bound fenced cursors, subject-owned progress, immutable receipts, stale-writer rejection and explicit reconciliation; endpoint changes and expiry still require current-state checks |
| Graph-assisted retrieval | Implemented in 0.13.0; experimental | Immutable server profiles, four reproducible anchors, strongest typed path, exact eligible revisions, separate score contributions and coverage; legacy cosine/hybrid contracts preserved |
| External graph retrieval comparison | Measured in 0.13.0 | Frozen document-only graph and E5 vectors, 100 reserved public-development queries, eight methods and 2,400 requests; primary gain remains uncertain and important category regressions prevent default admission |
| Additional relation evidence | Implemented in unreleased source | Non-endpoint context binds exact revisions and digest; invalidation affects reads, traversal and graph search; history, scope and work bounds remain enforced |
| Asynchronous consolidation | Proposed | External workers resume from checkpoints; late or duplicate work cannot publish stale relations; record partial or unknown execution without inventing completion |
| Graph-backed learned-tool reuse | Proposed | Exact immutable artifact and executor identity, verified environment applicability, cancellation, idempotency and isolated execution; a matching graph node alone cannot authorize execution |
| Evidence-based policy improvement | Proposed | Reuse the existing qualification and suspension authority; freeze policy and evaluator versions; retain failure evidence and compare fresh tasks before publication |

General graph schema definitions are still volatile and low-level transactions
can bypass high-level constraints. Tenant memory integration must not treat those
interfaces as a production authorization or global integrity boundary. Its
contract needs explicit scope and revision checks at the actual serving path.

## Evaluation design

Freeze memories, source revisions, extraction outputs, model identities and
relevance judgments. Separate development queries from held-out evaluation.
Use the same external embeddings across vector, hybrid and graph-assisted modes.
Include exact identifiers, error codes, paraphrases, Portuguese text, ambiguity,
contradictions, temporal changes, duplicate evidence and queries with no answer.

Compare the existing lexical, vector and hybrid baselines with graph expansion.
Ablate relation types, traversal depth, source weighting and consolidation one at
a time. Report nDCG, recall with judgment coverage, useful-answer rate and
per-category regressions. Also report extraction cost, ingest latency, storage,
candidate and edge counts, response bytes, retrieval p50/p95 and end-to-end cost.
Graph construction is part of the cost, even when performed asynchronously.

The existing hybrid reports remain authoritative for their frozen experiments.
They do not establish that graphs will repair the observed regressions. Preserve
those queries and require a measured held-out benefit before changing defaults.

Functional tests must cover identical nodes and vectors across companies,
private subjects and resource scopes; revoked credentials; changed, rejected,
deleted and expired sources; stale consolidators; crash recovery; and cut-off
traversals. Empty or partial results must identify their coverage rather than
implying that the corpus had no relevant evidence.

After retrieval evaluation, compare agent tasks while holding model, prompt,
tools, permissions, context budget and grading fixed. Measure task completion
through observable effects, repeated mistakes, abstention, retention, calls,
tokens, time and cost. Keep failures, incomplete outcomes and uncertainty in the
report. Better rankings and an attractive graph visualization cannot substitute
for this task-level evidence.
