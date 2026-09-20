# Durable evaluation admission

The registered-procedure ledger records the evaluator's submitted outcome before
it can contribute to qualification. `admit_evaluation` requires the policy's exact
evaluator subject and retains the authenticated subject and credential identity.
A network adapter must independently authorize `procedure_evaluate` on the exact
resource scope; JSON cannot supply the authenticated actor.

## Outcomes

| Admission outcome | Meaning | Updates efficacy statistics? |
| --- | --- | --- |
| `accepted` | Complete compatible paired measurements accepted by the existing learning rule | Yes |
| `rejected` | Evaluator rejection, wrong contract/baseline, reused evidence or invalid evidence/phase | No |
| `incomplete` | Incomplete evidence or missing utility measurements | No |
| `cancelled` | Authenticated evaluator reports a cancelled experiment | No |
| `pending_or_unknown` | Experiment outcome is not known | No |
| `unknown_consumption` | A reportedly complete experiment lacks cost or latency accounting | No |

An accepted evaluation is not necessarily a promoted procedure: an accepted
negative observation can contribute to a final `Rejected` decision or monitoring
suspension. An admission rejection is distinct from that statistical procedure
state. Unmeasured outcomes leave the current procedure state and counters
unchanged; they do not count as monitoring success or failure. Consumers must
inspect these outcomes rather than infer that unchanged state proves a recent
execution succeeded. Cancellation is an evaluator report, not an executor stop
acknowledgement; no execution gateway exists in this API.

## Submission and identity

A submission contains `case_id`, `phase` (`Qualification` or `Monitoring`),
`policy_id`, `context_id`, exact `baseline_revision`, `evidence_ref`, `status`,
nullable `baseline_utility`, `candidate_utility`, `candidate_cost_units`,
`candidate_latency_ms`, and optional `detail`. Status is `complete`, `rejected`,
`incomplete`, `cancelled` or `pending_or_unknown`.

Policy/context/baseline must match the candidate's immutable binding. Null or
omitted accounting remains unknown; it is never converted to zero. Missing utility
is incomplete. Finite but invalid utility ranges are rejected as evidence.
Malformed/unrepresentable inputs and unauthorized actors fail before admission.
Text identities support 1–512 bytes, evidence/detail 1–2048 bytes; unknown JSON
fields fail strict decoding.

The case ID is immutable across phases and outcomes within a registered
procedure. Identical retries by the same evaluator subject return the original
receipt, even after credential rotation or later decisions. Changed submissions
conflict. A corrected experiment needs a new case ID; a previously accepted
evidence reference cannot be reused as a new independent case. The database
checks reference identity, not semantic independence or truth of remote traces.

## Single decision authority and recovery

Admission preparation calls the existing `LearningMemory` evaluation transition.
For accepted evidence, its admission receipt, evaluation receipt, evidence index
and any procedure decision commit in one synchronous WAL batch under the shared
writer lock. There is no second promotion implementation or cross-database
transaction. Non-admitted outcomes persist only their admission receipt.

`admission(tenant, namespace, procedure, case)` retrieves the original submission,
authenticated actor, explicit outcome/reason, state at admission and optional
accepted evaluation receipt. Reads reject unsupported identity/schema or a
mismatch with the underlying evaluation ledger. Tests exercise immutable retries,
contract mismatches, unavailable measurements, promotion and reopen recovery.
