# From retrieval to experience-driven memory

Status: **research-informed design**, capability map reviewed September 23, 2026. This document
maps the supplied research to implemented foundations, remaining capabilities
and acceptance evidence. Experience receipts are available in 0.7.0; revision-bound
derived memories are available in 0.8.0. Structured experience-backed candidates are available in 0.10.0; extraction
remains external. The 0.14.0 evidence-bound knowledge API adds revision-bound
proposals and current source checks during inspection and selection. Discovery replay
and retrofitting source bindings onto legacy strategy records remain proposals. This document does not
reproduce third-party experiments or establish autonomous improvement.
The [0.6.0 retrieval report](scifact-results.md) measures retrieval separately.
The [graph memory evidence map](graph-memory-evidence.md) extends this review with
the subsequently supplied graph research and the 0.12.0 durable graph
foundation, while keeping memory integration and quality evaluation explicit.

## What the supplied research contributes

| Source | Evidence reviewed | Implication to investigate |
| --- | --- | --- |
| [AI Meets Brain, v1](https://arxiv.org/html/2512.23343v1) | Survey of memory types, management, evaluation and security; not a QilbeeDB experiment | Distinguish experience, factual claims and procedures; evaluate their lifecycles separately |
| [ReasoningBank, v1](https://arxiv.org/html/2509.25140v1), with [Google's explanation](https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/) | Extracts reusable strategies from self-judged successes and failures; its consolidation appends memories | Preserve failure evidence and extraction provenance; an agent's judgment should remain distinguishable from an independently observed outcome |
| [ZenBrain, v1](https://arxiv.org/html/2604.23878v1) | Layered memory, retention and consolidation experiments; raw retrieval and judge-based answer metrics yield different rankings, and BM25 wins some aggregate metrics | Test routing, forgetting and consolidation by ablation; a biological analogy does not establish an engineering advantage |
| [Dream-RSI, v1](https://arxiv.org/html/2609.14858v1) | Replays recorded discovery trees to select exploration policies while holding the underlying agent and evaluator fixed | Record replayable attempts and their cost; evaluate policy versions on historical branches before new online trials |
| [Perplexity Brain announcement](https://www.perplexity.ai/en-GB/changelog/brain-faster-computer-models-website-publishing) | Product description of a private context graph, source links and overnight refresh | Evaluate source-linked consolidation and explicit user control; product-reported improvements are not independently reproduced evidence |

The supplied arXiv and [alphaXiv](https://www.alphaxiv.org/abs/2512.23343) links
identify the same work, not two independent confirmations. The supplied
[Perplexity blog](https://www.perplexity.ai/hub/blog/self-improving-memory-for-agents)
could not be retrieved on this review; the table uses its accessible official
announcement instead. The
supplied [SSRN paper](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=6617061),
*Agent Brain: A Biologically Inspired Memory System for Autonomous AI Agents,
with Head-to-Head Evaluation on LongMemEval*, was identified through indexed
metadata, but its full text returned HTTP 403. Its methods and results have not
been assessed here; another retrieval attempt on this review was unsuccessful.
The supplied [ExplainX article](https://explainx.ai/blog/google-deepmind-dream-rsi-recursive-self-improvement-2026)
is secondary coverage of Dream-RSI. Engineering decisions use the primary paper
linked above rather than treating that coverage as an independent experiment.
A search-results page is discovery material, not evidence.

## Platform boundary

QilbeeDB should preserve the evidence and authority needed to learn from
experience. External workers perform model inference, embedding generation,
strategy extraction and isolated execution. Lightweight clients submit and read
versioned records. Provider credentials do not need to enter the database.

The current [memory API](../api/versioned-memory.md) supplies durable, scoped,
revisioned records; [retrieval](../api/hybrid-memory.md) supplies explicit ranking
identities. The [learning API](../api/procedural-learning.md) owns qualification
and selection under immutable policy/context definitions. The
[tool API](../api/learned-tools.md) currently stores artifacts, executor profiles
and development outcomes for compatibility. It does not dispatch or sandbox
programs. Future knowledge capabilities follow the
[external tool ownership boundary](../architecture/learned-tools.md): applications
own code, packages and operational execution; QilbeeDB owns scoped knowledge and
evidence. New knowledge contracts must not require code or executor registration.
These are foundations, not an implemented reasoning-memory loop.

## Implementation map

The engineering hypotheses below are QilbeeDB proposals inferred from the
research. A paper's reported improvement is not an acceptance result for this
platform. Keep the existing retrieval comparison and the agent-task comparison
as separate experiments.

| Research input | Foundation already available | Remaining engineering work | Evidence required before claiming benefit |
| --- | --- | --- | --- |
| ReasoningBank: reusable strategies from successful and failed attempts | Immutable experience observations, structured strategies and the separate 0.14.0 source-aware knowledge path | Evaluate extraction and reuse on real tasks; legacy strategy records do not acquire mutable-memory source bindings automatically | Compare retrieval alone, success-only strategies and success-and-failure strategies with fixed task conditions; report repeated mistakes and task completion, including incomplete runs |
| Dream-RSI: exploration over recorded discovery history | Parent-event bindings, bounded ancestry, version identities and exact observation exports | Define a frozen replay manifest and an external evaluator with prefix-limited visibility, versioned policy code and explicit unsupported transitions | Repeated replay must agree on decisions and accounting; a selected policy must face fresh online trials against fixed exploration under the same compute budget |
| ZenBrain: routing, retention and consolidation | Scoped lexical, vector and hybrid retrieval; validity, review and transitive dependency checks | Evaluate routing and retention policies separately; preserve required counterexamples when consolidating | Ablate one mechanism at a time under equal storage/context budgets; measure old-task retention, retrieval quality and downstream task outcomes separately |
| AI Meets Brain: memory lifecycle and security | Separate memory, experience, procedure and tool contracts with scoped access | Specify lifecycle transitions and evidence obligations between those contracts | Exercise contradictory, stale, poisoned and revoked evidence; verify that a derived claim cannot silently become an authenticated observation or approved procedure |
| Perplexity Brain: source-linked, refreshed private context | Source revisions, transitive invalidation, change feeds and consumer checkpoints | Build a resumable external consolidation consumer; define audience and origin tracking for later shared releases | Restart and retry without duplicate effects; source changes suppress stale conclusions; copied evidence does not count as independent corroboration |

### Implemented source-aware knowledge path

The 0.14.0 [knowledge contract](../agent-memory/evidence-bound-knowledge.md)
records immutable proposals with exact memory revisions and external tool
identity declarations. It preserves original receipts while checking current
source validity separately from qualification. Updating, deleting, rejecting or
expiring a source can make qualified knowledge unavailable for reuse without
erasing its audit history. A valid receipt is not a lease on source validity.

The implemented selection v3 contract adds a scope-bound active index
and the server-owned `active_knowledge_bound_v1` ordering. An unresolved higher
candidate produces an incomplete decision instead of silently selecting a lower
one. Explicit learning and dependency budgets bound work; they do not define
agent autonomy. Version 2 retains its existing contract. Deployment and client
opt-in must be verified separately from source availability. Selection v4 also
supports explicit `memory_only` and `experience_memory` origins through
`active_knowledge_origin_bound_v1`; it does not infer support in older clients.

These capabilities address evidence lifecycle and bounded selection. They do not
implement a replay evaluator, autonomous hypothesis generation or measured
cross-agent learning. Preserve the source/qualification checks when evaluating
future consolidation; compare actual task benefit separately from functional
correctness. The research implications below remain hypotheses until those
experiments are completed.

### Delivery order

The current 0.10.0 work on verified history, audits and recoverable consumer
errors supplies reliable evidence transport. It does not implement strategy
learning. The 0.10.0 [strategy API](../api/experience-strategies.md) connects immutable
experience observations to structured candidates and the existing qualification,
selection and suspension authority. The next learning cycle must demonstrate
task benefit. For mutable memory dependencies, use the separate
[evidence-bound knowledge API](../agent-memory/evidence-bound-knowledge.md),
which checks exact source revisions at inspection and selection. This does not
retrofit existing experience-strategy records or prove extraction quality.
Workers remain external and executions remain isolated.

Replay follows that evidence and admission contract. It should not activate
policies solely because they score well on development history. Routing,
consolidation and retention then enter as separate ablations so that their
contribution can be measured rather than hidden in a simultaneous redesign.

Each research-driven feature PR should record the source and version, the
engineering hypothesis, the existing contract it extends, its observable failure
cases, and its validation evidence. Distinguish contract tests from quality
experiments. A quality report must identify the frozen baseline, candidate,
evaluation population, resource budget, uncertainty and regressions. Update this
map and the exported user guides as those capabilities become available.

## Existing foundations and proposed extensions

The [experience receipt API](../api/experiences.md) implements scoped attempts,
immutable context and parent-event bindings, authenticated observations and
consumption accounting. It records evidence declarations without independently
verifying external effects. Existing optional artifact bindings verify locally
stored tool bytes and pin exact observation identities; they must not become a
prerequisite for knowledge about externally owned tools. An interface-schema
identity is not proof of executable implementation identity. Generic
[derived records](../api/derived-memory.md) also bind exact source revisions and
check transitive eligibility. The [strategy API](../api/experience-strategies.md) now binds structured candidates
to immutable experience observations. Its observations do not count as
qualification trials. The separate evidence-bound knowledge API already binds mutable memory revisions
and checks their current validity during selection. New combined proposals bind
both evidence types explicitly; legacy strategy records are not retrofitted.
Discovery replay, shared release and outcome-driven retention remain proposals.
Each extension needs its own validated feature PR.

| Capability and status | Durable contract | Acceptance criteria |
| --- | --- | --- |
| Experience receipt (implemented foundation) | Attempt ID, parent attempt, exact context and artifact digests, authenticated reporter and declared evidence, resource consumption and idempotency key | Retried reports do not duplicate attempts; unknown completion or consumption remains unknown; scope and crash recovery preserve receipts |
| Derived strategy and combined knowledge (implemented foundations) | Exact experience observations and, for combined proposals, memory revisions; extractor identity, preconditions, counterexamples and candidate revision | A summary cannot silently become a verified observation; bound memory changes and authorization affect subsequent eligibility checks |
| Discovery replay (proposed) | Immutable attempt graph, recorded transitions, visibility at each step, frozen utility/cost formula and exact replay-engine version | Replay cannot inspect future scores or invent unobserved transitions; repeated runs yield the same decisions and accounting |
| Shared knowledge release (proposed) | Immutable candidate, evidence lineage, explicit audience, policy/context identity and a publication decision owned by the existing learning authority | Cross-tenant and cross-subject access fails before retrieval; copies of one source are not counted as independent corroboration; suspension reaches subsequent selections |
| Retention policy (proposed) | Versioned retention decision, reason, evidence obligations and eligible record classes | Storage savings are measured alongside old-task retention; counterexamples survive consolidation when required; deletion semantics are explicit |

For the proposed replay contract, an unavailable branch returns an explicit
unsupported transition. It must not receive a fabricated score or be counted as
a real failed execution. A replay score is a development signal; promotion needs
fresh online evidence under the registered context. This is an engineering
inference from the restricted historical support in Dream-RSI, not a guarantee
that arbitrary counterfactual policies can be evaluated off-policy.

## Shared memory without self-reinforcing errors

The proposed shared layer should track the origin of evidence across agent
transfers. Ten agents repeating one summary still have one underlying source.
Disagreement should preserve the conflicting claims, their applicable times and
their environment versions. Agreement, frequent retrieval and model confidence
must remain separate from demonstrated task utility.

Generic derived memories already enforce transitive source eligibility before
direct reads and lexical, vector and hybrid retrieval, including lexical corpus
statistics. Source edits, deletion, expiry and review revisions make dependent
records ineligible on subsequent request snapshots. The
[derived-memory contract](../api/derived-memory.md) documents graph and work
bounds, snapshot semantics and the absence of descendant change events.

The implemented [combined knowledge contract](../agent-memory/evidence-bound-knowledge.md)
binds exact experience observations and current memory-source revisions in one
versioned proposal. It preserves the existing qualification authority and does
not retrofit older strategy records. Source-aware selection checks current
eligibility separately from immutable evidence history.

[External graph consolidation](../api/graph-consolidation.md) also provides durable
jobs, credential-bound leases and atomic assertion publication for external
workers. These are implemented coordination contracts, not an autonomous replay
evaluator or evidence of improved reasoning. Future work on shared publication
and replay must preserve the documented authorization and snapshot boundaries.
Dependency metadata does not discover undeclared copies, erase external caches
or retain historical source payloads. A shared release must not promise those
properties implicitly.

## Evaluation before automatic publication

Freeze the task sequence, models, prompts, tools, permissions, budgets and grader.
Compare no memory, current retrieval, success-only strategies, success-and-failure
strategies, and replay-selected exploration under the same resource accounting.
Randomize or repeat task orders to measure sensitivity to accumulated history.
Keep development history separate from future tasks and independent evaluation.

Measure task completion with observable effects, repeated mistakes, adaptation
after an environment change, retention of earlier tasks, abstention, tokens,
execution calls, wall time and total cost. Include contradictory evidence,
unreliable judges, interrupted workers, poisoned source content and revoked
access. Report failures and incomplete runs alongside successful runs.

Forgetting and consolidation require matched storage/context budgets and
component ablations. Replay needs a comparison with a fixed exploration policy
and fresh online execution. Learning across agents needs a comparison with
isolated agents and equal total compute. Retrieval nDCG cannot substitute for
any of these outcomes.

Experience receipts and revisioned memory-source bindings provide the knowledge
foundation. Existing tool artifact records are compatibility capabilities, not a
requirement for future external-tool knowledge. Structured strategy candidates now bind exact experience evidence to that
qualification authority. Current source-aware inspection and selection are implemented in the separate
evidence-bound knowledge API. Controlled task evaluation remains necessary;
qualification and current source validity are separate checks, and neither proves
that externally reported outcomes actually occurred. Replay
and shared release depend on those contracts. Automatic selection can operate
within an authorized policy after qualification; unmeasured candidates remain
candidates, and a valid baseline remains available when evidence is rejected or
incomplete.

Bounded history, pinned ancestry and exact observation exports are also
implemented. These preserve evidence for a future replay evaluator; they do not
execute replay or select exploration policies. See the
[experience evaluation workflow](experience-evaluation.md).
