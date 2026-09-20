# Procedures bound to registered contracts

`LearningMemory::propose_registered(tenant, namespace, request, actor)` resolves
an immutable policy and context inside the supplied tenant. The request contains
only `id`, `policy_id`, `context_id`, `instructions` and `source_refs`. Task,
baseline and promotion criteria come from the registered contracts. Their
evaluation-contract identities must match.

## Identity and immutable retries

The procedure ID is unique within the authenticated tenant and namespace.
Reusing it with changed instructions, provenance, policy ID or context ID fails
with a conflict, even when a new context ID contains identical context fields.
A revision must use a new procedure ID. An identical retry returns the original
`ProposalReceipt`, including the original registering actor, contract digests
and initial candidate record.

The internal learning scope hashes the complete authorized namespace and the
exact policy/context IDs and digests. Model, tools, environment, permissions,
baseline and evaluator-contract changes therefore do not inherit a previous
qualification state. Hashes partition records; the receipt also retains the
original namespace and immutable references for verification and inspection.

## Atomic storage and current state

Candidate and receipt are written in one RocksDB batch with a synchronous WAL.
Concurrent identical proposals produce one receipt. `registered_procedure`
returns that original receipt alongside the current `ProcedureRecord`, preserving
its decision history. Reads verify the binding, registry digests, immutable
proposal, scope and creation timestamp; a missing or inconsistent record fails
explicitly instead of appearing as an unqualified candidate.

Registration and evaluation use the same learning ledger. There is no second
promotion authority. The lower-level Rust APIs remain trusted embedding
interfaces; they must not be directly exposed to an untrusted proposer.

## Authorization and evidence boundary

The caller must derive tenant, actor and namespace from current authentication.
The namespace includes project, mission, agent, visibility and private subject
where applicable. Source references record declared provenance; the database
does not claim to have resolved or independently verified arbitrary external
references. Held-out evaluation and authenticated evaluator admission follow
in the procedural API integration.

This increment adds the storage contract, not an HTTP endpoint or an executor.
