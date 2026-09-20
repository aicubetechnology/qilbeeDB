# From retrieval to experience-driven memory

Status: **research-informed design**, reviewed September 20, 2026. This document
maps recent work to testable QilbeeDB extensions. The initial experience receipt contract is implemented in the
0.7.0 API; other extensions below remain proposals. This document does not
reproduce third-party experiments or establish autonomous improvement.
The [0.6.0 retrieval report](scifact-results.md) measures retrieval separately.

## What the supplied research contributes

| Source | Evidence reviewed | Implication to investigate |
| --- | --- | --- |
| [AI Meets Brain, v1](https://arxiv.org/html/2512.23343v1) | Survey of memory types, management, evaluation and security; not a QilbeeDB experiment | Distinguish experience, factual claims and procedures; evaluate their lifecycles separately |
| [ReasoningBank, v1](https://arxiv.org/html/2509.25140v1), with [Google's explanation](https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/) | Extracts reusable strategies from self-judged successes and failures; its consolidation appends memories | Preserve failure evidence and extraction provenance; an agent's judgment should remain distinguishable from an independently observed outcome |
| [ZenBrain, v1](https://arxiv.org/html/2604.23878v1) | Layered memory, retention and consolidation experiments; raw retrieval and judge-based answer metrics yield different rankings, and BM25 wins some aggregate metrics | Test routing, forgetting and consolidation by ablation; a biological analogy does not establish an engineering advantage |
| [Dream-RSI, v1](https://arxiv.org/html/2609.14858v1) | Replays recorded discovery trees to select exploration policies while holding the underlying agent and evaluator fixed | Record replayable attempts and their cost; evaluate policy versions on historical branches before new online trials |
| [Perplexity Brain announcement](https://www.perplexity.ai/en-GB/changelog/brain-faster-computer-models-website-publishing) | Product description of a private context graph, source links and overnight refresh | Evaluate source-linked consolidation and explicit user control; product-reported improvements are not independently reproduced evidence |

The supplied arXiv and alphaXiv links for `2512.23343` identify the same work,
not two independent confirmations. The supplied Perplexity blog could not be
retrieved; the table uses its accessible official announcement instead. The
supplied [SSRN paper](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=6617061),
*Agent Brain: A Biologically Inspired Memory System for Autonomous AI Agents,
with Head-to-Head Evaluation on LongMemEval*, was identified through indexed
metadata, but its full text returned HTTP 403. Its methods and results have not
been assessed here. A search-results page is discovery material, not evidence.

## Platform boundary

QilbeeDB should preserve the evidence and authority needed to learn from
experience. External workers perform model inference, embedding generation,
strategy extraction and isolated execution. Lightweight clients submit and read
versioned records. Provider credentials do not need to enter the database.

The current [memory API](../api/versioned-memory.md) supplies durable, scoped,
revisioned records; [retrieval](../api/hybrid-memory.md) supplies explicit ranking
identities. The [learning API](../api/procedural-learning.md) owns qualification
and selection under immutable policy/context definitions. The
[tool API](../api/learned-tools.md) stores artifacts, executor profiles and
development outcomes. It does not yet dispatch or sandbox executable programs.
These are foundations, not an implemented reasoning-memory loop.

## Proposed contracts and acceptance criteria

The [experience receipt API](../api/experiences.md) implements scoped attempts,
immutable context and parent-event bindings, authenticated observations and
consumption accounting. It records evidence declarations without independently
verifying external effects. Explicit artifact bindings now verify locally stored
tool bytes and pin exact observation identities. Remaining objects below
are design proposals, not current endpoints or accepted request fields. Each
extension needs its own validated feature PR.

| Extension | Proposed durable contract | Required acceptance evidence |
| --- | --- | --- |
| Experience receipt | Attempt ID, parent attempt, exact context and artifact digests, observed outcome, verifier identity/version, resource consumption and idempotency key | Retried reports do not duplicate attempts; unknown completion or consumption remains unknown; scope and crash recovery preserve receipts |
| Derived strategy | Source record IDs and revisions, extractor identity, success/failure classification, preconditions, counterexamples and candidate revision | A summary cannot silently become a verified observation; source edits, revocation and expiry invalidate affected serving decisions |
| Discovery replay | Immutable attempt graph, recorded transitions, visibility at each step, frozen utility/cost formula and exact replay-engine version | Replay cannot inspect future scores or invent unobserved transitions; repeated runs yield the same decisions and accounting |
| Shared knowledge release | Immutable candidate, evidence lineage, explicit audience, policy/context identity and a publication decision owned by the existing learning authority | Cross-tenant and cross-subject access fails before retrieval; copies of one source are not counted as independent corroboration; suspension reaches subsequent selections |
| Retention policy | Versioned retention decision, reason, evidence obligations and eligible record classes | Storage savings are measured alongside old-task retention; counterexamples survive consolidation when required; deletion semantics are explicit |

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

Source invalidation must reach derived strategies as well as direct search hits.
An admission check should validate the current dependency set before serving a
derived release. Tests should race revocation with consolidation and ensure the
documented authorization boundary holds. This propagation is proposed work;
the present source-bound vector invalidation does not implement a general
derivation graph or complete knowledge erasure.

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

Experience receipts now provide the first bounded implementation. Stored tool artifact bindings are also implemented. The next steps
are revisioned memory-source bindings and candidate strategy derivation. Replay
and shared release depend on those contracts. Automatic selection can operate within an authorized
policy after qualification; unmeasured candidates remain candidates, and the
baseline remains available when evidence is rejected or incomplete.

Bounded history, pinned ancestry and exact observation exports are also
implemented. These preserve evidence for a future replay evaluator; they do not
execute replay or select exploration policies. See the
[experience evaluation workflow](experience-evaluation.md).
