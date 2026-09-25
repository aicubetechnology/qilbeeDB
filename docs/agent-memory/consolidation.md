# Memory consolidation

Use consolidation to retain relationships and reusable instructions derived from
existing evidence. QilbeeDB provides durable contracts for externally generated
candidates; enabling the database does not start an autonomous reasoning or
replay process.

## Choose a consolidation contract

| Your goal | Contract | What QilbeeDB preserves |
| --- | --- | --- |
| Extract relationships from memories | [External graph consolidation](../api/graph-consolidation.md) | Bounded jobs, credential-bound leases, exact source revisions and atomic publication of typed assertions |
| Retain instructions derived from experience observations | [Experience-backed strategies](../api/experience-strategies.md) | Immutable observation references and a procedural candidate subject to qualification |
| Bind instructions to current memory validity, optionally with experience evidence | [Evidence-bound knowledge](evidence-bound-knowledge.md) | Exact memory revisions, explicit origin, immutable receipts and current eligibility checks |

These contracts apply to both managed-platform clients and self-hosted servers
that support the documented API versions. Use your service endpoint and a
credential with the capabilities and scope required by the chosen contract.

## Preserve evidence through the workflow

1. Select authorized source records and freeze the required revisions or exact
   experience observations.
2. Run extraction in your application or worker. Keep its model, method and
   external tool references explicit. If generation uses memories as well as
   experience observations, use the combined knowledge contract to bind both.
3. Submit the result through the matching contract. Graph publication creates
   assertions, while a knowledge proposal creates a candidate for the existing
   qualification process. Neither operation establishes that its content is true.
4. Inspect current source eligibility before reuse. An immutable receipt remains
   historical evidence even when an update, deletion, rejection or expiration
   makes its sources ineligible.

For graph jobs, follow the documented lease, retry and recovery rules. Preserve
an uncertain publication's original identity and inspect its state before
retrying; do not create a new operation merely because a response was lost.

## Limits and responsibilities

The application or worker owns model inference and candidate generation.
QilbeeDB owns authorization, persistence, source bindings and the contract's
publication or qualification checks. A selected procedure does not authorize
external execution.

These APIs do not implement autonomous discovery replay or prove that
consolidation improves retrieval or agent tasks. Evaluate those outcomes
separately. Do not infer automatic migration between short-term and long-term
memory, automatic forgetting, or model execution from the term “consolidation.”
