# Evaluate experience evidence

Use immutable experience observations to preserve the inputs to an agent-task
comparison. This workflow is available in the 0.7.0 API. It produces reproducible
evidence exports; it does not demonstrate that a strategy improves an agent.
Check the deployed version with `/health` and the exported guide's release stage.

## Define the comparison before collecting outcomes

Register an immutable evaluation context that identifies the task, baseline,
external model revision, tool versions, environment, evaluator contract, dataset,
harness and permissions. Keep these conditions fixed when comparing memory or
exploration methods. Use a new context when a condition changes.

Define the task population, baseline/candidate pairing, success criteria, cost
unit, stopping rule and treatment of interruptions before looking at results.
Keep development tasks separate from held-out evaluation tasks. Store these
protocol definitions externally under immutable references and hashes; an
experience evidence declaration alone does not make a protocol preregistered or
independently verified.

The database checks equality of supplied context identities and accounting units.
It cannot infer whether an external worker used the declared model, followed the
protocol, selected representative tasks or produced independent observations.

## Record successes, failures and incomplete work

Create one [experience attempt](../api/experiences.md) for each execution. Bind
its reporter subject to the authorized observer, then append observations with
an exact revision and an evidence reference. Report unknown completion as
`unknown`; leave unknown consumption null. Zero is a measured or asserted zero,
not a replacement for a missing value.

If an execution later completes, append its terminal observation. A terminal
outcome cannot change, but the same terminal outcome can receive later accounting
updates. Preserve the earlier event IDs and hashes. Do not rewrite a failed run
as a successful retry: register a new attempt and, where appropriate, pin the
original failure as its parent.

Use explicit artifact bindings when stored tool bytes form part of the evidence.
A binding verifies the stored artifact identity, not that execution occurred.
Lineage preserves an exact historical branch; it does not supply counterfactual
outcomes for branches that were never executed.

## Freeze the selected observations

Choose an explicit event for each selected attempt. Retain its `attempt_id`,
`event_id` and `event_digest` in the evaluation manifest. Freeze this selection
before comparing outcomes, together with the context digest, accounting unit,
task pairing and cohort-selection procedure. Save the server revision from the
deployment metadata as well as the API version.

Call `POST /api/v1/experiences/export` with that selection. Each request accepts
1–64 distinct attempts within one authorized scope and one context/accounting
unit. The service rejects incompatible or missing members as a whole. It sorts
the returned events by attempt ID, reports the server-owned export method and
computes an opaque digest of the export. Save the complete response, not only its
digest: a digest identifies evidence but cannot reconstruct missing bytes.

For larger cohorts, partition a frozen manifest into deterministic batches of at
most 64 attempts and retain every batch response and digest. Check for duplicate
attempts across batches in the external evaluator; server deduplication applies
to each request. Define cross-scope comparisons explicitly and authorize each
scope separately. There is no implicit global scan or cross-tenant cohort.

An export can be reproduced after later events arrive or the server restarts by
submitting the same exact selection. Replacing a selected event with a later
accounting correction intentionally produces a different export. Retain both
versions and document the reason for the update.

## Interpret the summary without discarding missing data

Report the denominator of selected attempts and all four outcome counts. A
success rate among completed tasks and a success rate among all selected tasks
answer different questions; name the denominator and disclose unknown outcomes.
The service exports counts and deliberately does not choose a statistical
estimator, utility formula, acceptance threshold or publication decision.

Reported consumption totals remain null when any selected report is unknown.
The observed lower bound retains known cumulative consumption even if a later
report omits a value. It is a lower bound, not an estimate of missing consumption.
Totals use decimal strings so sums beyond unsigned 64-bit range remain exact.
Compare latency distributions from individual events; a sum does not describe
p50/p95 latency or parallel elapsed time.

Repeated trials, duplicated source content and multiple artifact links do not
establish statistical independence. Equal context digests do not establish
random assignment. Preserve task IDs and pairing externally, inspect failure
categories, and report uncertainty with the assumptions required by the chosen
estimator. Observational cohort selection can introduce bias even when every
export hash is correct.

## Confirm effects before publication

Historical exports support auditing and development. A replay-selected strategy
still needs fresh, authorized online trials under the frozen evaluator. Keep the
same model, prompts, tools and acceptance criteria when measuring the contribution
of memory. Compare observable task completion, repeated errors, calls, tokens,
latency and consumption, including incomplete runs.

The existing [procedural learning authority](../api/procedural-learning.md)
continues to own qualification and selection. Experience exports do not bypass its
evidence rules or automatically activate a procedure. Retrieval ranking metrics
and synthetic contract tests are separate from evidence of improved agent-task
performance. The [research design](experience-memory-design.md) describes the
implemented source-revision and [strategy candidate](../api/experience-strategies.md)
foundations, plus remaining mutable-source eligibility and replay work.
