# Evidence-bound procedural knowledge

Status: **API and core available in 0.14.0**. Qualification covers real HTTP
contracts, recovery and bounded selection. The corporate console candidate has a
separate publication lifecycle. Functional qualification does not demonstrate
better agent reasoning or transfer execution authority to the database.

## Purpose and responsibility

Use evidence-bound knowledge to retain instructions supported by exact memory
revisions. The application owns tools, code, operational permissions and execution.
QilbeeDB records knowledge, external identity declarations and observed evidence.
It does not download references, inspect installed executables or run tools.

Qualification and current evidence validity are separate. An active procedure can
become ineligible when a source changes, expires, is rejected or is deleted.
Historical receipts remain immutable. A successful inspection is a time-bounded
observation, not a reservation or permission to execute external actions.

## Core operations

All three operations use POST with `contract_version: 2` and an authorized `scope`.
The `/api/v1` routing prefix is retained. Responses to successful requests use
version 2; errors retain the platform's version 1 error envelope.

| Endpoint | Authority | Result |
| --- | --- | --- |
| `/api/v1/learning/knowledge/proposals` | `procedure_propose` and `memory_read` | Original immutable receipt |
| `/api/v1/learning/knowledge/inspect` | `memory_read` | Current qualification and source observation |
| `/api/v1/learning/knowledge/select` | `memory_read` | Complete compatible selection or explicit baseline |

Tenant and private-subject identity come from authentication. Registering code or
an executor, or holding a tool-administration capability, is not a prerequisite.
Inspection and selection share retrieval admission after authorization. Responses
use `Cache-Control: no-store`.

## Read-only client configuration

A client that reads policy and evaluation-context definitions before selection
needs `learning_metadata_read` in addition to `memory_read` with exact memory
resource grants. The metadata capability permits GET policy/context reads for
its authenticated company. **It is company-wide**: project, agent, mission and
private-subject memory grants do not restrict these registry definitions.
Grant it only when this visibility is intended, such as an isolated benchmark
company. A product requiring project-isolated metadata needs a separate contract.

This capability does not permit policy/context registration, proposals,
evaluations, memory writes, reviews or credential administration. Existing
`policy_admin` credentials retain their read behavior; existing credentials do
not gain the new capability automatically. Do not add `policy_admin` to a
read-only evaluator as a workaround. Revocation and expiry still apply.

## Immutable proposal

Supply `id`, `policy_id`, `context_id`, `title`, `instructions`, `memory_sources`
and `external_tools`. A proposal uses a new ID for every changed revision. Existing
procedures cannot acquire source bindings retroactively.

Sources contain unique `record_id` values and exact positive `revision` values.
The accepted range is 1–16 roots. Source revisions are unsigned 64-bit integers;
clients must preserve them without floating-point rounding. Creation can record
currently unavailable sources, but cannot qualify or validate them by doing so.

Each external tool contains `name`, `schema_revision`, `implementation_revision`,
`environment_revision` and `usage_contract`. Names are unique; at most 32 tools
are accepted. Implementation/environment must both be explicit strings or both
explicit nulls. Omission is not equivalent to null. Null means implementation
identity is unbound; it is not a wildcard. Schema identity must match the existing
evaluation context's tool entry. An explicit environment must match that context.
The application supplies appropriate implementation identities for code-specific
evaluations; a schema digest cannot prove executable identity.

Identity strings and title are limited to 512 UTF-8 bytes, instructions to 32768,
and each usage contract to 4096. The complete HTTP request still has a 65536-byte
limit. These are protocol bounds, not company execution quotas.

Source arrays are canonically ordered by UUID and tool arrays by name before
comparison and hashing. Reordering those arrays preserves an exact retry. Duplicate
names or source IDs are rejected. Changed content under the same ID conflicts.
The original actor and initial candidate record remain in the receipt after retries.
After a lost response, keep the original intent and retry exactly; do not silently
create a new ID. The interoperability vectors preserve unsigned 64-bit revisions, explicit
null identities, Unicode and escaped text. Run
`python3 scripts/check_knowledge_digest_fixture.py` to independently check its
canonical proposal and original receipt bytes and SHA-256. The native suite
consumes the same vectors. Fixture validation does not establish current source
validity, client integration acceptance or arbitrary JSON canonicalization.
Receipt digests are server-produced integrity identifiers, not signatures or
permissions. Preserve the entire original receipt when recording a retry outcome;
do not substitute current qualification state or the retrying actor into it.

## Inspect before reuse

Inspection accepts `procedure_id`. The response contains the immutable `receipt`,
current `procedure`, `qualification_active`, `evidence` and
`eligible_for_knowledge_reuse`. Evidence reports its observation time, work,
coverage and first failure. The first failure is not a complete list of issues.

Reuse eligibility requires active qualification AND complete valid source evidence.
Transitive source checks share a snapshot; expiry is checked even without a feed
event. Corruption or exhausted dependency I/O returns an error, not an empty or
successful partial observation. The current graph bounds are 64 unique nodes and
8 levels per candidate, with 4096 distinct dependency records and 16 MiB loaded
payload bytes per selection request.

## Select compatible knowledge

Supply `policy_id`, `context_id`, `max_instruction_bytes` (0–65536),
`candidate_limit` (1–1000), and required `external_tool_identities`.
The latter contains the identity fields of the proposal's tools, excluding
`usage_contract`. Matching is exact and independent of array order. Empty matches
only empty; unbound implementation identity matches only unbound identity.

The selector checks compatibility and current evidence before ranking, retaining
the existing improvement-bound ordering and deterministic procedure-ID tie-break.
Invalid candidates cannot hide a valid lower-ranked candidate.

The response's `result` contains `selection`, `evaluated_at_millis` and `coverage`.
Coverage reports `records_examined`, `candidates_eligible`, `complete` and
`stop_reason`. The candidate bound counts scanned procedure records, including
inactive records. A truncated scan returns the registered baseline with reason
`selection_incomplete`; it never declares a partial winner. A complete scan with
no eligible candidate returns `no_eligible_bound_procedure`.

There is no cursor. Repeating a query creates a new live observation. Persistent
incompleteness requires diagnosis and future index/query evolution; this contract
does not promise unlimited history scale. A baseline response identifies the
registered baseline and does not independently authorize its execution.

## Compatibility and remaining validation

Legacy selection excludes v2-bound records; v2 selection excludes unbound v1
records. Existing historical reads and evaluations retain the same qualification
authority. Consumers must adopt v2 explicitly, without a silent fallback to v1
following a v2 error or incomplete observation.

Before release, qualification must include real HTTP/OpenAPI responses, canonical
native/Python vectors, hard-kill recovery, source invalidation, isolation, bounded
coverage and the human administration journey. Functional tests alone establish
neither better retrieval nor improved agent reasoning.

## Company administration

An administrator with `credential_admin` can discover retained knowledge across
company workspaces and private subjects through
`POST /api/v1/company/learning/query`, using contract version 1 and query kind
`knowledge`. The company is derived from the authenticated credential; the request
cannot select another company. Writer revocation does not remove retained records
from this inventory.

Use the returned `resource` selection unchanged. Its title describes the original
knowledge proposal, while its status reflects the current qualification ledger.
Neither inventory status nor the historical details returned by
`POST /api/v1/company/learning/read` proves that source evidence remains usable.

To check current evidence, send `contract_version: 2` and the selected `resource`
to `POST /api/v1/company/learning/knowledge/inspect`. The response includes
`company_id`, `resource` and the same `inspection` structure used by scoped
inspection. This endpoint accepts only the `knowledge` resource kind. It does not
issue credentials, register executors or grant permission to execute a tool.

Catalog pages are live observations. Follow `next_cursor` with the same kind and
filters; a partial or empty filtered page does not establish that the company has
no knowledge. Keep already confirmed rows after a transient continuation failure
and offer retry. If a current-evidence refresh fails, clear any prior readiness
indicator rather than presenting stale evidence as current. Authentication or
permission failures must clear inaccessible details. Present private ownership,
workspace context and incomplete coverage visibly; keep raw resource identifiers
in advanced details.

### Read knowledge in the corporate console

In the unreleased console candidate, open **Learning & policies**, choose
**Evidence-bound knowledge**, and select a title. The catalog discovers authorized
company records; routine inspection does not require a scoped API key or manually
entered project, agent or memory identifiers.

The details panel separates **Qualification** from **Source evidence**. An active
qualification can remain historically correct while the current observation says
**Not eligible for reuse**. The panel explains the first source failure and shows
when it checked the evidence. Deleting a source also advances its revision, so an
exact revision binding may report a changed revision before a deleted-source
reason. Both observations prevent reuse; neither changes the historical receipt.

**View evidence history** opens the existing registered procedure's qualification
ledger. Knowledge does not create a second evaluation history. Historical cases
remain inspectable even after a source changes or the writer loses access; their
presence does not restore current eligibility. Return to the current resource and
refresh its evidence before making a reuse decision.

**Refresh details** obtains a new observation. An unavailable refresh removes the
previous eligibility display and offers **Retry inspection**. A revoked session
clears protected rows and details. Keyboard focus stays in the inspector during
refresh, and Escape returns to the selected row. Source identifiers and revisions
are available under diagnostic details. An origin is not inferred from synthetic
scores or resource names; absent an explicit origin field it is shown as
**Not provided**.

This console candidate remains under release validation. Its observation does
not authorize application execution or demonstrate agent reasoning improvement.
