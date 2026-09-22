# Discover company learning resources

**Availability: unreleased.** These endpoints are being qualified for the next
release. They are not part of the deployed 0.13.0 API. Check the running server's
`/openapi.json` before using them.

Company administrators can discover retained experiences, procedures, strategies,
tool artifacts, development requests, policies, evaluation contexts and executor
profiles without knowing their identifiers in advance. Discovery reads the
learning ledger directly: a project does not need a memory record to appear.
The original writer's expiration or revocation does not hide retained company
records from its authorized administrator.

This is an administrative inventory. A listed resource is not automatically
approved, safe to execute, fresh agent context, or evidence of better reasoning.
Use the [procedural learning contract](procedural-learning.md) for qualification
and the [learned-tool contract](learned-tools.md) for lifecycle operations.

## Authorization

Both endpoints require a currently valid company credential or login session
with `credential_admin`. The server derives the company from that credential;
the request cannot select another company. Exact project grants are unnecessary
for this company-wide administrative view. Private resources retain their
project, agent, mission and owner, including different subjects with the same
resource identifier.

Company-wide policy, context and executor records have no workspace scope.
Scoped resources must have a canonical platform namespace. Records created
through the trusted Rust library with another namespace are counted as skipped;
the catalog does not guess their project or private owner. This contract does
not import those records or alter their existing library access.

These reads do not issue delegated keys, register agents, change resource state,
or generate embeddings. Tool code is returned only when its selected artifact is
inspected; the database never executes it during discovery.

## Find and select a resource

Send `POST /api/v1/company/learning/query` with a resource kind:

```json
{
  "contract_version": 1,
  "query": {
    "kind": "experience",
    "limit": 25,
    "max_scanned_records": 100
  }
}
```

Supported kinds and the origin of their displayed title:

| Kind | Displayed title | State |
| --- | --- | --- |
| `experience` | Task in the registered evaluation context | `unreported`, `unknown`, `succeeded`, `failed`, or `cancelled` |
| `procedure` | Task in the current registered procedure | `candidate`, `active`, `rejected`, or `suspended` |
| `strategy` | Extracted instructions | Current state of its bound procedure |
| `tool_artifact` | Entrypoint | `registered`; registration does not certify safety |
| `tool_development` | Requested objective | Current development state |
| `policy` | Registered policy identifier | `registered` |
| `context` | Registered task | `registered` |
| `executor` | Registered executor identifier | `registered`; a profile does not prove isolation |

Titles use existing recorded text, normalize whitespace, and retain the first
160 Unicode characters. They are not unique names. Use each returned `resource`
selection, including its scope and private subject, to distinguish equal titles
and identifiers. `recorded_at_millis` describes the original registration time,
not the last refresh or last lifecycle change. Mutable experiences and development
requests also expose their current revision.

The response includes `company_id` and a `page` with `entries`. Each entry has
`resource`, `title`, `status`, `revision`, and `recorded_at_millis`. To inspect an
entry, copy its entire `resource` object into `POST /api/v1/company/learning/read`:

```json
{
  "contract_version": 1,
  "resource": {
    "kind": "experience",
    "id": "attempt-42",
    "scope": {
      "project_id": "research",
      "agent_id": "analyst",
      "mission_id": null,
      "visibility": "private"
    },
    "private_subject_id": "researcher"
  }
}
```

The server derives the physical namespace again from the authenticated company
and the selection; neither a selection nor a cursor grants access. A successful
read returns `details.kind` and its typed `details.record`. A strategy returns
both its immutable candidate receipt and its current bound procedure. A missing
selection returns `404 record_not_found`. Re-read when opening a detail view:
the state may have changed since the list was observed.

## Filter and continue discovery

For scoped kinds, `query.filter` accepts exact `project_id`, `agent_id`,
`mission_id`, `visibility`, and `private_subject_id` filters. An omitted or null
filter means any value; a non-null mission selects that exact mission. These
scope filters are rejected for company registry kinds. Scope filters are applied
before fetching and verifying the selected resource's dependency records.

All kinds accept `filter.text`, a case-insensitive substring of the displayed
title or resource ID. This is a catalog filter, not full-text, semantic or hybrid
retrieval. It does not search tool source code or undisplayed instructions.

Continue by sending the returned `page.next_cursor` unchanged as `query.cursor`,
with the same kind and filters. Reset the cursor when those selections change.
The page size and scan budget can change during continuation. Cursors are bound
to the company, kind and exact filter values. A changed or malformed binding
returns `400 invalid_request`.

Traversal follows stable internal key order, not relevance, creation time or
alphabetical display order. The cursor advances after the last scanned primary
record, including records omitted by filters or non-platform namespaces. An
empty page with a cursor is incomplete discovery, not an empty company. Each
request observes consistent learning state while holding the learning writer
lock. Pages do not share a snapshot: later inserts behind the cursor require a
new traversal, and existing resources can change between pages.

## Understand coverage and work limits

| Field | Meaning |
| --- | --- |
| `stop_reason` | `exhausted`, `entry_limit`, `scan_limit`, or `byte_limit` |
| `next_cursor` | Continuation when a limit stopped this page; null when the remaining prefix was exhausted |
| `scanned_records` | Primary resource records examined on this page, including filtered/skipped entries |
| `scanned_record_bytes` | Bytes in the examined primary keys and values |
| `skipped_non_platform_records` | Examined scoped records that lack a canonical platform namespace |
| `observed_at_millis` | Server time at the start of this serialized observation |

The default page limit is 25, with a maximum of 50 entries. The default scan
budget is 100 primary records, with a maximum of 1,000. A fixed 4 MiB primary
key/value ceiling can stop a page earlier; a single primary record above this
ceiling fails the catalog request. Resume from the supplied cursor to avoid
discarding progress at an empty or truncated page.

Byte counters exclude dependency verification, RocksDB internals and any
lookahead, and are not response-size, CPU or memory measurements. Existing
kind-specific readers verify immutable bindings, digests and current state;
strategy verification also checks its bounded cohort of source observations.
These checks may read more data than the primary scan counter reports. Requests
share the server's retrieval admission limit and briefly serialize with learning
writes. Measure workload latency before increasing page or scan budgets.

Integrity failures return an error for the whole request, never a partial
successful list. `exhausted` means the remaining requested kind/company prefix
was traversed at that observation; it is not a total-company count or a claim
that every retained item is reusable.

## Handle errors and recovery

| HTTP status | Expected handling |
| --- | --- |
| `400` | Correct invalid fields, bounds, scope shape or cursor binding. Restart traversal when filters change. |
| `401` | Authentication is missing, expired or revoked. Clear protected data and authenticate again. |
| `403` | Company administration is not granted. Do not infer an empty inventory. |
| `404` on read | The selected resource is unavailable in this company. Refresh the list. |
| `413` | Reduce the request; these endpoints use the 65,536-byte body limit. |
| `500` | Integrity or storage failure. Do not use a partial or previous response as current state. |
| `503 retrieval_busy` | Retry with bounded backoff and the same cursor after capacity becomes available. |

Authentication and administrative authorization precede retrieval admission.
Every response uses `Cache-Control: no-store`. On a failed continuation, retain
the last confirmed cursor and explicitly label any displayed earlier page as a
previous observation. Do not advance progress until the page has been accepted.

## Interface integration and current boundaries

A management interface should open with a resource list, let the administrator
select an entry, and preserve project/agent/owner context in its detail view.
Keep raw identifiers and exact contract JSON in advanced details. Show loading,
empty-filtered, incomplete, failed and successful states distinctly, and provide
keyboard operation, predictable focus and a path back to the list.

This inventory covers the eight parent resource kinds above. The unreleased
[learning evidence history](learning-evidence-history.md) adds bounded discovery
of evaluation submissions, paired comparisons, experience observations and
development events for a selected parent. Memory consumer checkpoint discovery
remains separate product work. Qualify each private console journey against the
exact deployed endpoints before claiming that its management screens are complete.
