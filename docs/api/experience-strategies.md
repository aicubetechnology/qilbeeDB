# Experience-backed strategy candidates

Available in **0.10.0**. Register externally extracted instructions with their
applicability conditions and the exact experience observations used to derive
them. QilbeeDB validates the observations and creates a procedural candidate in
the same durable transaction. Qualification and suspension use the existing
[learning authority](procedural-learning.md).

This bounded capability is inspired by
[ReasoningBank](https://arxiv.org/html/2509.25140v1): preserve successes and failures
as strategy-development evidence. It does not run an extraction model, reproduce
the paper's quality results or automatically qualify a strategy.

## Prerequisites and authorization

Register an immutable policy and evaluation context, then record the development
[experiences](experiences.md). Freeze one observation per selected attempt using
its exact `attempt_id`, `event_id` and `event_digest`. Each observation includes
the historical experience revision; later observations cannot rewrite it.

Creation requires both `procedure_propose` and `experience_read`. Reading a
strategy receipt requires both `memory_read` and `experience_read`. Each
capability must grant the exact project, mission, agent and visibility. Tenant
and private subject come from the current credential. Authorization applies to
retries and historical reads, including after credential revocation.

## Register a candidate

Call `POST /api/v1/learning/strategies`:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "strategy": {
    "id": "receipt-recovery-v1",
    "policy_id": "policy-v1",
    "context_id": "context-v1",
    "instructions": "Read the operation receipt before deciding whether to retry.",
    "preconditions": ["The operation provides a durable receipt lookup."],
    "counterexamples": ["A timeout alone does not establish that the operation failed."],
    "extractor": {
      "provider": "external-provider",
      "model": "external-extractor",
      "model_revision": "model-v1",
      "prompt_revision": "extraction-v1",
      "evidence_ref": "trace://development/receipt-recovery"
    },
    "selection": {
      "context_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "accounting_unit": "tokens",
      "events": [{
        "attempt_id": "development-failure-1",
        "event_id": "observed-failure",
        "event_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      }]
    }
  }
}
```

Replace the illustrative IDs and hashes with registered records and returned
digests. Invented or mismatched observation references are rejected.

| Field | Contract |
| --- | --- |
| `id` | Immutable candidate/procedure identity, 1–512 UTF-8 bytes; reuse unchanged for retry |
| `policy_id`, `context_id` | Existing immutable contracts with a matching evaluation contract |
| `instructions` | Externally produced text, nonblank, at most 16,384 UTF-8 bytes |
| `preconditions` | 1–16 nonblank applicability statements, each at most 1,024 UTF-8 bytes |
| `counterexamples` | 0–16 nonblank limitations or failure cases, each at most 1,024 UTF-8 bytes |
| `extractor` | Provider, model, model revision and prompt revision identities, each nonblank and at most 512 UTF-8 bytes; evidence reference at most 2,048 bytes |
| `selection` | 1–16 distinct attempts from one authorized namespace, matching the exact registered context digest and one accounting unit |

The standard 64 KiB limit applies to the complete encoded request, including JSON
escaping and whitespace. Individual field limits do not guarantee that every
combination fits. Unknown request fields are rejected.

Outcomes and consumption come from the resolved immutable events. Success,
failure, cancellation and unknown observations are all retained. Unknown outcome
is not failure; unknown consumption remains null. The database does not verify
external execution, extraction quality or the truth of a reporter's assertion.

## Receipt, retries and durability

The response is `{contract_version: 1, receipt}`. The receipt contains:

- `method_version: "qilbee.experience-strategy.v1"` and the original request;
- `export_digest` and the exact observation set's outcome/accounting `summary`;
- `proposal`, the registered-procedure receipt in initial `Candidate` state;
- `strategy_digest`, identifying the request, export, summary and proposal binding.

Strategy evidence, the proposal binding and the initial procedure record commit
atomically in one synchronous WAL batch. Failed validation writes none of them.
Identical retries return the original receipt after restart, later observations,
qualification or suspension. Event-list order is part of request identity even
though the evidence export sorts its events. Changed content under an existing
ID returns 409; use a new ID for a revised strategy. An existing ordinary
procedure cannot be retroactively converted into a strategy candidate.

The procedure instructions use a server-owned JSON representation with
`format: "qilbee.strategy-instructions.v1"`, `instructions`, `preconditions` and
`counterexamples`. These are the exact bytes evaluated and subsequently selected.
Its `source_refs` contains a digest reference to the complete strategy request,
the extraction evidence reference, and the selected observations' input and
outcome evidence references. The existing authority rejects direct reuse of
these references as evaluation evidence. Read the strategy receipt to inspect
the exact event bindings. Renaming a reference cannot establish independence.

Call `POST /api/v1/learning/strategies/read` with:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "strategy_id": "receipt-recovery-v1"
}
```

Reading revalidates the stored digest, exact observation export and original
procedure binding. The proposal remains the original candidate receipt. Use
`/api/v1/learning/procedures/read` for current qualification and selection state;
the receipt is not a present eligibility assertion.

## Evaluate and select the same candidate

Submit held-out paired trials through `/api/v1/learning/evaluations`, using the
strategy ID as `procedure_id` and the registered policy/context identities. The
designated evaluator must use its own authorized credential. Strategy creation
adds zero qualification trials. Development observations do not become paired
evaluations and their outcomes do not establish improvement.

Incomplete trials and unknown consumption retain the existing admission outcomes.
Only the existing policy can qualify a candidate. Selection uses
`/api/v1/learning/select`; monitoring can suspend it through the same authority.
Keep the baseline available when selection returns no eligible procedure.

Freeze the task split, model, prompts, tools, grader and budgets before comparison.
Report task effects, repeated mistakes, calls, tokens, latency and failures. The
database checks registered identities; the external evaluator must establish
that held-out tasks are independent of development observations. A source digest
does not prove independence. See [experience evaluation](../research/experience-evaluation.md).

## Errors and current limits

| Status | Meaning and action |
| --- | --- |
| 400 | Invalid shape/version, empty or oversized fields, or duplicate attempts; correct the request |
| 401 / 403 | Missing current credential, capability or scope grant; restore authorized access |
| 404 | Missing source event, strategy, policy or context in the authorized namespace |
| 409 | Changed immutable ID, conflicting ordinary proposal, or evidence/context mismatch; inspect the binding and create a new candidate when appropriate |
| 413 | Encoded request exceeds 64 KiB |
| 500 | Inconsistent persisted evidence; do not treat partial metadata as a valid candidate |

This contract binds immutable experience observations. It does not bind arbitrary
mutable memory records to procedure eligibility. The separate
[derived-memory API](derived-memory.md) enforces source revisions for memory
retrieval; propagating their eligibility into strategy selection remains future
work. Preconditions and counterexamples are preserved instructions, not
executable predicates or automatically verified proofs.

Replay execution, cross-agent evidence independence and retention-policy learning
remain separate extensions. Contract and synthetic lifecycle tests validate
behavior; they are not a measured gain in agent-task performance.
