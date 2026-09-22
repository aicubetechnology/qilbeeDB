# Name registered agents for human navigation

**Availability: 0.14.0.** Confirm the routes in your server’s `/openapi.json`
before integration; older installations require an upgrade. The existing
[`GET /api/v1/agents`](agent-registration.md) response remains unchanged.

An agent's ID belongs to the application or company that operates it. A display
name helps a person recognize that ID in a directory, a memory filter or a graph.
It does not replace the ID, transfer ownership, grant permissions, certify an
agent's status or alter its memories. Different agents may have the same name;
show the underlying ID as secondary information when choosing between them.

Only agents already observed after a successful authorized resource request can
receive a name. Naming an unknown ID returns `404 record_not_found`; it does not
create an agent. First registration, its source scope and its author remain
immutable. Configure an [application integration](company-integrations.md) and
let the application send its first authorized request when registration is
needed. A profile is optional: an existing agent without one is still registered.

## Complete the task in the administration console

In a compatible console, sign in as a company administrator and open **Agents**.
Search by name or external ID, select the intended agent, and edit **Display
name**. Choose **Save name**, or leave the field blank and choose **Clear name**.
The current name, external ID and first registration remain visible together.
Expand **Name change history** to inspect saved changes and their authors.

A failed next page must preserve the confirmed entries and their continuation.
Use **Continue discovery** for a partial page and **Clear search** for a filter
that finds no match. The directory's first project is registration provenance;
it is not a complete list of the agent's projects or current access.

The private console candidate preserves the exact pending name command and actor
in session storage before sending it. If that record cannot be preserved, no
write is sent. **Review pending change** and **Check saved change** reconcile a
lost response against server history, then read the current profile separately.
The record survives reload and sign-out in the same tab and is presented only
after the same company account authenticates again. It contains the proposed
name and identifiers, but no authentication secret. Closing the tab or clearing
its storage loses this local recovery record without canceling a server write;
use server history and administrative support if recovery information is lost.
This is not a cross-device operation inbox.

Name discovery in memory and graph selectors and learning-resource context keeps
the original IDs in requests. The candidate resolves only discovered IDs, with
at most four name reads in flight. Refresh retries unavailable names. The Agents
screen loads up to 25 matches per page, examines up to 100 registrations per
request and retains at most 500 matches; narrow the search at that display limit.
A console release requires its own deployment validation; API availability alone
does not establish that these controls are installed.

## Authorization and scope

All four endpoints require a currently valid company credential or login session
with `credential_admin`. The server derives the company from that authority.
Do not send a company, subject or physical namespace in the request. Exact
project grants are not required for this company administration workflow.

A name applies to one external agent ID throughout that company, including its
projects and private subjects. The same ID in another company has a separate
profile and history. A writer's expiration or revocation does not remove the
registration or label; the administrator's own authority must still be valid.

## Discover and inspect an agent

Send `POST /api/v1/company/agents/query`:

```json
{
  "contract_version": 1,
  "query": {
    "text": "research",
    "limit": 25,
    "max_scanned_agents": 100
  }
}
```

Omit `text` or use null to browse every registered agent. Text is a
case-insensitive substring of the current display name or external ID; it is not
semantic retrieval. The default result limit is 25, with a maximum of 100. The
primary scan budget defaults to 100 and cannot exceed 1,000 registrations.

Each entry includes the existing registration fields and `profile`. A null
profile means the agent has never received a display-name command. A profile
whose `display_name` is null means its name was explicitly cleared. Both states
need a clear “Not named” presentation; neither means “Not registered”. A failed
or incomplete name lookup must instead say that the name is unavailable or not
yet loaded.

Select an entry and send `POST /api/v1/company/agents/read`:

```json
{
  "contract_version": 1,
  "agent_id": "research-agent"
}
```

Read the current profile before editing. Use its revision for the command below,
or zero when `profile` is null. Never infer a revision from a displayed row count,
a registration timestamp or an earlier receipt.

## Save or clear a name

Send `POST /api/v1/company/agents/commands`:

```json
{
  "contract_version": 1,
  "command": {
    "agent_id": "research-agent",
    "expected_revision": 0,
    "display_name": "Research assistant",
    "idempotency_key": "rename-research-agent-2026-09-21-01"
  }
}
```

Names must contain 1–256 UTF-8 bytes, have no control characters, and have no
surrounding whitespace. They are stored exactly as supplied. The field is
required: explicitly send null to clear a name. An omitted field is not a clear
instruction. IDs and command keys also use the existing 1–256-byte identifier
validation. Company IDs and agent IDs are never generated by this operation.

The expected revision must match the current profile. Every accepted command
advances it once, including an explicit repeated value or clear operation.
Current profile, immutable history and replay receipt commit together through a
synchronous write-ahead log. Concurrent administrators using the same revision
cannot both win. A stale revision returns `409 revision_conflict`.

The response contains `result.receipt` and `result.replayed`. The receipt includes
the exact command and the resulting profile, including its revision, server
commit time and authenticated actor. Repeating the same company-bound command
key, body and actor returns that receipt without applying another change. A
changed command or actor returns `409 idempotency_conflict`.

## Recover an unconfirmed change

A lost response does not establish failure. Keep the original command, key,
expected revision and actor identity. Retry that exact command while the same
authenticated actor remains authorized; do not assign a new key to retry it.

After a successful replay, read the agent again. A valid receipt may describe a
previous revision if another administrator has since edited the name. Show the
confirmed operation and the current state separately.

If the login session expired or the credential changed, the new actor may no
longer qualify for the original replay. Use the authorized history endpoint to
inspect the revision immediately after the original expected revision. Match
its complete command and original actor before declaring the operation saved.
If a different command occupies that revision, refresh the current profile and
let the person explicitly choose a new change. Do not automatically overwrite it.

If no matching history is observed and the original operation could still be
pending, retain the uncertain outcome. An absent receipt at one observation does
not prove that an in-flight write cannot commit later.

## Inspect the name history

Send `POST /api/v1/company/agents/history`:

```json
{
  "contract_version": 1,
  "agent_id": "research-agent",
  "after_revision": 0,
  "limit": 25
}
```

The page returns ascending immutable receipts, `current_revision` and an optional
`next_after_revision`. Pass the continuation unchanged for the same company and
agent. The limit defaults to 25 and accepts 1–100. An `after_revision` ahead of
the current profile returns `409 revision_conflict`; reconcile the selected agent
and server history before continuing. A never-named registered agent has revision
zero and an empty history. Later commands may appear on subsequent pages.

## Pagination and current observations

The directory traverses external IDs in UTF-8 byte order. Its cursor is bound to
the company and exact text filter; result and scan limits can change. It advances
after the last examined registration, including one that did not match the name
filter. An empty page with a cursor is incomplete discovery, not an empty company.

Directory pages and history pages are live observations, not a snapshot across
requests. Each request serializes with memory metadata mutations. Newly inserted
agents or newly matching names behind the directory cursor require restarting
traversal. `scanned_agents` counts primary registrations examined on that page;
it does not measure all integrity reads, bytes, CPU, memory or response size.
The directory checks the current profile's history and replay bindings while
assembling each entry. Inconsistent stored companions fail the entire request.

## Errors and interface acceptance

All requests use the 65,536-byte body limit and all responses disable caching.
Authentication and administrative authorization precede retrieval admission for
directory, current-read and history requests. These reads may return
`503 retrieval_busy`. Profile commands use the memory mutation serialization
path and do not consume a retrieval permit.

| Status | Action |
| --- | --- |
| `400` | Correct invalid fields, name bytes, missing clear intent, bounds or cursor binding. |
| `401` / `403` | Clear protected data and recover authorized access; do not infer an empty directory. |
| `404` | The ID is not registered in this company. Discover the correct agent or complete its first application request. |
| `409 revision_conflict` | Inspect current state and require a deliberate decision before issuing another change. |
| `409 idempotency_conflict` | Reconcile the original command and actor; do not generate another retry key. |
| `413` | Reduce the request body. |
| `500` | Storage or integrity failure. Do not display an old profile as freshly confirmed. |
| `503` on reads | Keep the last confirmed continuation and retry with bounded backoff. |

A management console must support discovery, selection, a labeled name field,
current-state feedback, keyboard return and recovery without routine manual IDs.
Keep registration provenance and full receipts available through additional
details. Display the name first, with a secondary ID and an explicit missing-name
state. Validate conflicts, unknown responses, session loss, partial catalogs,
mobile layout and keyboard navigation before publishing that human workflow.
This API guide does not claim that a console release has completed those checks.
