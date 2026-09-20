# Immutable learning contracts

The Rust `LearningMemory` registry stores administrative policy and exact
experimental context independently of a procedure proposal. QilbeeDB owns this
contract and requires no QMN service.

## Policy authority

`register_policy(tenant, id, definition, actor)` accepts one explicitly named
algorithm: `fixed_budget_hoeffding_v1`. Its parameters are the validated
`LearningPolicy`: fixed qualification count, one-sided error budget, minimum
improvement/utility, resource limits, monitoring failure limit, evaluator subject
and evaluation contract. This policy is not equivalent to QMN sign tests or
family-wide error spending. Its error budget applies to one proposal; registering
many proposals does not establish a family-wide guarantee.

A revision uses a new identifier. Repeating an identical definition returns the
original registration receipt, including its original actor and timestamp.
Changing a definition under the same tenant/identifier conflicts. No registry
operation activates a procedure or claims its evidence is true.

## Exact context

`register_context(tenant, id, context, actor)` preserves these exact identities:

| Field | Meaning |
| --- | --- |
| `task` | Versioned task definition |
| `baseline_revision` | Exact baseline used for paired comparison and fallback |
| `model_provider`, `model_revision` | Provider and immutable model/deployment identity |
| `tools` | Map of tool name to exact artifact/version identity; empty means no tools |
| `environment_revision` | Runtime/environment identity |
| `evaluation_contract` | Rubric and experimental protocol |
| `dataset_revision` | Held-out dataset/case generation identity |
| `harness_revision` | Evaluator implementation identity |
| `permissions_revision` | Execution permission policy identity |

Each identity contains 1–512 UTF-8 bytes. The tool map supports at most 128
entries; these are technical contract bounds, not retention quotas. Unknown
JSON fields and unsupported algorithm names are rejected. A model or permission
change requires a new context, rather than inheriting an old validation result.
Callers must supply actual stable identities; the database cannot resolve an
arbitrary provider label into an immutable external model by itself.

## Receipts and storage

`policy(tenant, id)` and `context(tenant, id)` return immutable registry entries
with schema version, tenant, identifier, payload, SHA-256 payload digest,
registering actor and transaction timestamp. Reads reject unsupported schemas
and identity/digest mismatches explicitly. SHA-256 covers typed JSON serialization
with deterministic tool-map ordering; it is a consistency check, not an external
signature or proof that the underlying experiment happened.

Registry mutations share the learning store's writer lock and acknowledge only
after synchronous WAL writes. Concurrent identical registrations return one
original receipt. Identical identifiers in different tenants remain isolated.
Tests cover conflicting retries, concurrent writers, strict decoding, corrupted
records and reopen persistence.

## Integration boundary

These blocking Rust interfaces are trusted storage operations. Network adapters
must derive tenant and actor from a live credential, require `policy_admin` for
registration, and resolve immutable references before creating a proposal.
This increment does not itself expose registry HTTP routes or execute code.
