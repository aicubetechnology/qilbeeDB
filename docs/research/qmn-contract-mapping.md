# QMN contract mapping for an independent QilbeeDB

Initial source inspection: September 20, 2026, `qilbee-ecosystem` revision
`3d107a8f393a3657c5bcaeb0b8ef7755be3db53e`. QMN is a migration reference, not a
runtime dependency. This inventory records inspected behavior and gaps; it does
not claim that the reference system's complete suite was run or that all its
historical advertised capabilities work.

## Procedural decisions are not interchangeable

| Contract | Inspected QMN behavior | Current qilbeeDB library | Integration requirement |
|---|---|---|---|
| Evaluation identity | Exact model/provider, environment, harness revision and permission digest | Host-supplied environment and evaluation-contract strings | Define explicit platform context fields and exact matching; no automatic efficacy transfer |
| Experiment registration | Immutable plan, task families, repetitions, suite digest, baseline revision and registered trial index | Immutable proposal with a fixed qualification budget | Preserve a preregistered experiment manifest before accepting results |
| Accuracy gate | Family-level sign test, no case regression and trial-index alpha allocation | Per-proposal Hoeffding bound over paired utility differences | Version the policy; never silently translate one decision into the other |
| Efficiency gate | Checks normalized usage, complete cost trace and setup/execution/verification/cleanup phases | Candidate cost/latency integers supplied by the evaluator | Unknown usage remains unknown; incomplete accounting cannot become zero cost or eligible evidence |
| Incomplete comparison | Rejects missing/duplicate slots and inconsistent reports | One paired case per receipt, candidate remains pending until its fixed count | Retain a distinguishable incomplete/rejected receipt and avoid promotion |
| Release recall | Tenant/project/channel/model context, approved artifact and exact experiment evidence | Active procedure selection by scope, baseline and contract | Use one durable publication/decision authority and an explicit compatibility adapter |
| Host execution | Recalled procedure text is historical data and grants no execution authority | Ledger stores instructions without executing them | Keep execution permission separate from knowledge eligibility |

Sources inspected:
[experiment contracts](https://github.com/aicubetechnology/qilbee-ecosystem/blob/3d107a8f393a3657c5bcaeb0b8ef7755be3db53e/services/qmn/services/shared/experiments.py),
[resource evidence](https://github.com/aicubetechnology/qilbee-ecosystem/blob/3d107a8f393a3657c5bcaeb0b8ef7755be3db53e/services/qmn/services/shared/resource_efficiency.py),
[execution context](https://github.com/aicubetechnology/qilbee-ecosystem/blob/3d107a8f393a3657c5bcaeb0b8ef7755be3db53e/services/qmn/services/shared/evaluation_context.py),
[release lookup](https://github.com/aicubetechnology/qilbee-ecosystem/blob/3d107a8f393a3657c5bcaeb0b8ef7755be3db53e/services/qmn/services/shared/releases.py),
[native procedure client](https://github.com/aicubetechnology/qilbee-ecosystem/blob/3d107a8f393a3657c5bcaeb0b8ef7755be3db53e/crates/qilbee-rust/src/qmn/procedures.rs).

## Migration method

Inventory each observable request, receipt, state transition and authorization
boundary before implementing its platform equivalent. Use the corresponding
reference tests as compatibility evidence, add platform contract tests, and
record deliberate incompatibilities. An imported artifact starts without
platform efficacy certification unless its evidence and exact policy/context
are verifiably supported; do not copy an `active` label as proof.

The reference repository has concurrent local work. Inspection is read-only;
capability migration work belongs in independent qilbeeDB features. Broader
memory, discovery, program, gateway and identity mappings remain to be inspected
and qualified under the [delivery acceptance contract](delivery-acceptance.md).
