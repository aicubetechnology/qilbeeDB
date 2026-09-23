# Company memory administration

**Availability:** 0.12.0 contract. These routes are not available in versions
through 0.11.0. Check `/health` and the installation's `/openapi.json` before
integrating them.

A company administrator can discover and inspect every retained platform memory
workspace in that company, including each private subject. Discovery reads a
catalog maintained from canonical memory storage. It remains available when the
credential that wrote a memory has expired or been revoked. It does not depend
on enumerating credential grants or issuing temporary data credentials.

This is an administrative view of retained state. Agent retrieval continues to
require its own operation capability, authorized resource scope, private subject,
and current memory eligibility. Administrative visibility does not approve a
memory or make it safe to reuse as agent context.

## Authenticate as a company administrator

Send a company credential or human login session with `credential_admin` in the
`Authorization: Bearer` header. No individual memory grant or `memory_read`
capability is required for these administrative routes. Grant this capability
only to administrators who should manage company access and inspect all company
memory, including private subjects.

The company comes from the authenticated credential. Requests cannot override
the company, private owner, or underlying namespace. A workspace ID is an opaque
identifier, not an access token. The same external project and agent IDs in two
companies remain separate. An integration credential with `memory_read` alone
receives 403. Revoked, expired, or otherwise invalid credentials receive 401.
The installation administrator obtains authority in the selected company through
the [company administration flow](global-administration.md).

Responses use `Cache-Control: no-store`. The reads do not issue credentials,
register agents, change reviews, activate journals, or advance consumer progress.
Authorization checks occur before retrieval admission. A revocation prevents new
authorization; an already authorized in-flight read can complete.

## Discover workspaces

```bash
curl --get "$QILBEEDB_URL/api/v1/company/memory/workspaces" \
  --header "Authorization: Bearer $QILBEEDB_COMPANY_ADMIN_TOKEN" \
  --data-urlencode 'contract_version=1' \
  --data-urlencode 'limit=25'
```

The response contains `contract_version`, `company_id`, and `page`. Each item in
`page.workspaces` contains:

| Field | Meaning |
| --- | --- |
| `workspace_id` | Opaque 64-character lowercase hexadecimal identifier |
| `company_id` | Authenticated company |
| `scope` | Exact project, agent, optional mission, and visibility |
| `private_subject_id` | Private owner for a private scope; null for shared memory |

To continue, send `page.next_after_workspace_id` as `after_workspace_id` and keep
the other parameters unchanged. Null means that traversal reached the end of the
current directory. The default page size is 25 and the maximum is 100. Ordering
is by workspace ID, with an exclusive cursor.

A workspace remains listed if all its records are deleted, expired, rejected, or
invalidated. The directory establishes retained storage membership; it does not
claim that an agent is active or that a workspace contains usable context. An
unused grant or a successful empty agent query does not create a memory workspace.
The [agent directory](agent-registration.md) records successful agent observations
and has a different purpose.

## List memories by creation date

**Availability:** additive, unreleased endpoints. Confirm both ordered routes in
`/openapi.json` before enabling a date-ordering interface. Existing UUID-based
query routes keep their current behavior. This contract is the same for the
managed platform and self-hosted installations that provide these endpoints.

Use `POST /api/v1/company/memory/query/ordered` to list across the authorized
company without downloading every workspace. The server orders records before
pagination. For a human memory explorer, default to **Newest first**, offer
**Oldest first**, and display the creation date with a recognizable time zone.

```json
{
  "contract_version": 1,
  "selection": {
    "project_id": null,
    "agent_id": null,
    "workspace_id": null,
    "visibility": null,
    "private_subject_id": null
  },
  "view": "current",
  "query": {
    "order": "created_desc",
    "limit": 40,
    "scan_limit": 1000,
    "cursor": null,
    "text_contains": null,
    "tag": null,
    "episode_type": null
  }
}
```

Null or omitted selection fields leave that field unrestricted within the
company. Non-null fields combine with AND. An exact workspace also selects its
mission. A private-subject selection narrows an administrator's existing access;
it cannot grant or override authority. `view` defaults to `retained`; choose
`current` to suppress records that are currently ineligible.

Each `page.entries` item contains its `workspace`, current canonical `record`,
and `eligibility`. The page also contains `next_cursor`, `stop_reason`,
`evaluated_at_millis`, `scanned_records`, `record_bytes`, and `dependency_work`.
The company is derived exclusively from authentication.

### Scoped application access

An application gateway with `memory_read` uses
`POST /api/v1/memory/query/ordered` with `contract_version`, its existing `scope`,
and the same `query` object. Its response has `scope` and `page.records`, plus the
same page metadata. It returns only currently eligible records in that exact
scope. Private ownership comes from the credential. It needs no company
administrator credential and cannot choose another private subject. Keep gateway
credentials on the server; do not expose them to a browser.

### Ordering, continuation, and changes

`created_desc` is the default; `created_asc` reverses it. The key is immutable
server `created_at_millis`, followed by workspace ID for company queries, then
record UUID. Descending reverses this complete tuple. UUIDs break timestamp ties;
event time and modification time do not determine the order. Updates, reviews,
and retained deletion markers keep their creation position.

Copy `next_cursor` unchanged into `query.cursor`. This opaque string is at most
1,024 ASCII bytes. It binds the endpoint, company or exact scope, selection, view,
order, and text/tag/type filters. Limits may change between pages. A cursor is a
seek position, not authorization, a snapshot, or evidence that a memory is still
safe to reuse. Restart the listing when changing filters or order. Invalid,
mismatched, and unsupported cursors return HTTP 400 `invalid_request`; no
clock-based cursor expiration is defined in this version.

Each page uses a coherent snapshot, but the entire traversal is live. Records
inserted before the continuation position require a refresh; records inserted
after it may appear later. Each page reevaluates current contents, filters, and
eligibility. Edits do not duplicate an immutable key already traversed. Refresh
clears both results and cursor; it does not append a new first page to old rows.
Before agent reuse, revalidate through the existing memory read APIs.

### Bounded work and recovery

Defaults are 40 results and 1,000 examined positions. Maximums are 100 results,
10,000 positions, and 8 MiB of selected canonical record bytes per request.
`scanned_records` counts traversed chronological index positions, including
selection exclusions and tombstones. Selection is checked before reading payloads.
Root byte counts exclude index values, dependency reads, lookahead, and serialized
response overhead; they are not a total memory or response-size guarantee.
Existing aggregate dependency limits still apply.

`stop_reason` is `exhausted`, `record_limit`, `scan_limit`, or `byte_limit`.
Only `exhausted` has a null continuation. An empty partial page means more
positions remain; it does not mean the company is empty. Continue with its cursor.
No total count or cross-page snapshot completeness is promised. A complete
response is globally ordered across selected workspaces, even when directory
selectors have not loaded every workspace.

Busy servers return HTTP 503 `retrieval_busy`; retry with bounded backoff.
Dependency-budget or integrity failures fail the whole request. Preserve the
last confirmed cursor and rows after a failed read. HTTP 401 requires renewed
authentication; HTTP 403 requires authorized scope/capability. Clear protected
rows when access is lost. Offer a visible refresh action to recover from an
invalid continuation, and never present a silently reordered partial page.

For self-hosted upgrades, chronological projections are derived from canonical
records before readiness. Bounded, synchronous batches make an interrupted
backfill restartable. Subsequent writes update the projection in the same atomic
batch as the record; older writers are detected by journal fingerprints on
reopen. Keep a consistent backup and budget startup time for the first backfill.

## Query retained records

Send `POST /api/v1/company/memory/query` with an ID returned by the directory:

```json
{
  "contract_version": 1,
  "workspace_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "filter": {
    "limit": 25,
    "scan_limit": 500,
    "after": null,
    "view": "retained",
    "text_contains": null,
    "tag": null,
    "episode_type": null
  }
}
```

The example ID is a placeholder. The response includes the resolved `workspace`
and a `page` with `entries`, `next_after`, `stop_reason`, `evaluated_at_millis`,
`scanned_records`, `record_bytes`, and aggregate `dependency_work`.
Each entry has a `record` and an `eligibility` explanation. Both describe the same
storage snapshot and server clock for that request.

Choose the view deliberately:

| View | Returned entries |
| --- | --- |
| `retained` (default) | Current canonical revisions, including deleted markers and ineligible records |
| `current` | Only records that pass current root and transitive source eligibility |

A deleted record has a null payload. Its identifier, revision, author, and retained
metadata remain inspectable. An expired or rejected record can retain a payload,
but its `eligibility.eligible` is false. Derived records can fail when an exact
source revision changes, is rejected, expires, or becomes unavailable. The
explanation reports the first failure; an early failure does not claim every
source was examined. See [source eligibility](../api/derived-memory.md).

This API reads the current retained revision. It does not recover overwritten
historical payloads or reconstruct a deleted body. Administrative results are not
a historical archive and must not be substituted for current agent retrieval.

Filters apply to payload content. `text_contains` is a case-insensitive substring
across primary, secondary, and context text, limited to 4096 UTF-8 bytes. `tag` is
an exact tag match, limited to 256 UTF-8 bytes. `episode_type` uses the existing
memory type representation. The filters combine with AND. Deleted markers match
only when all payload filters are absent or null. Text filtering is not semantic
search or a relevance ranking.

## Continue without losing progress

Records are traversed in UUID byte order. `next_after` is the last examined UUID,
which can belong to an excluded record. It is not necessarily the last returned
entry. Send it as `filter.after` while retaining the workspace, filters, and view.
Continue when it is non-null, even when `entries` is empty.

| Stop reason | Meaning |
| --- | --- |
| `exhausted` | No more retained roots in this workspace at the request's snapshot |
| `record_limit` | The response reached its requested entry limit and another root exists |
| `scan_limit` | The root examination budget was reached and another root exists |
| `byte_limit` | The next root would exceed the decoded root-byte budget |

A single request uses a consistent snapshot. Multiple pages use live snapshots,
so a concurrent insertion before the cursor requires a new traversal to discover
it. Updates can change eligibility and payloads between requests. The exclusive
cursor prevents revisiting the same UUID within a forward traversal; it does not
freeze a company-wide export. No total count or cross-page completeness at one
instant is implied.

## Inspect one retained record

Send `POST /api/v1/company/memory/read`:

```json
{
  "contract_version": 1,
  "workspace_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "record_id": "a37bd762-bf52-49f6-9a90-a4c4c41c3df5"
}
```

The response contains `workspace`, `entry`, and `record_bytes`, together with
`contract_version` and `company_id`. A retained tombstone returns 200 with a null
payload and an ineligible explanation. An absent record, absent workspace, or
workspace belonging to another company returns the same 404 `record_not_found`.
Ordinary agent reads continue to suppress deleted and ineligible records.

## Resource limits and errors

| Budget | Bound |
| --- | --- |
| Directory entries | 1–100; default 25 |
| Returned query entries | 1–100; default 25 |
| Examined query roots | 1–10000; default 500, including excluded roots |
| Decoded root bytes | 8 MiB per query or direct inspection |
| Transitive dependency work | Existing aggregate bound: 4096 records and 16 MiB per request |
| Per-root source walk | Existing bound: 64 ancestors and depth 8 |
| Request body | Existing 65536-byte non-vector transport limit |

The byte counters measure retained serialized roots and dependency payloads.
They exclude integrity indexes, directory metadata, and a fetched lookahead
value. They are not a bound on process memory, database I/O, or final JSON size.
Entry dependency counters report incremental reads; shared cached dependencies
are not counted twice. The page reports aggregate dependency work, including
records excluded by `view=current` after evaluation.

A root larger than 8 MiB cannot be served through this inventory and returns 400.
Aggregate dependency-budget exhaustion fails the entire request; reduce page or
scan limits. Integrity failures also fail the request, rather than returning a
partial successful inventory.

| Status | Handling |
| --- | --- |
| 200 | Inspect entries, eligibility, and continuation before interpreting coverage |
| 400 | Correct version, identifiers, unknown fields, filters, limits, or excessive work |
| 401 | Authenticate again or replace invalid authority |
| 403 | Use an authorized company administrator |
| 404 | Workspace or record unavailable in this company; applies to query and read |
| 413 | Reduce the request body |
| 500 | Storage or integrity failure; do not infer an empty company |
| 503 `retrieval_busy` | Shared retrieval capacity is occupied; retry with bounded backoff |

All three routes share the configured retrieval admission pool with other memory
reads and searches. Permits cover the blocking inventory operation and response
serialization. See [retrieval capacity](../operations/retrieval-capacity.md).

## Upgrade and verification

At startup, the existing canonical memory scan backfills catalog membership for
platform namespaces, including workspaces containing only tombstones. Subsequent
memory mutations commit catalog membership in the same synchronous WAL-backed
batch as their records. An interrupted upgrade can resume this idempotent work.
Account for startup scanning and metadata writes when planning an upgrade.

The existing namespace encoding, memory IDs, revisions, receipts, and private
ownership remain unchanged. Trusted library callers using arbitrary non-platform
namespaces are outside this company catalog. Malformed canonical platform
addresses or inconsistent catalog metadata fail closed. The catalog can be
rebuilt from retained canonical keys; credential metadata is not its authority.

Qualification includes revoked-writer discovery, two private subjects, identical
external IDs in two companies, migration from uncatalogued retained data,
filtered and byte-limited continuation, eligibility, failed mutation atomicity,
corruption detection, real HTTP/OpenAPI validation, admission exhaustion, and
acknowledged writes surviving a killed server process. This validates the
administrative contract; it does not measure retrieval relevance or agent ability.

## Read current evidence ancestry

The 0.12.0 `POST /api/v1/company/memory/graph` route accepts a workspace ID
and root memory UUIDs. It returns eligible nodes and revision-bound evidence
edges in one snapshot, with explicit depth/node cuts. It uses the same company
administrative authority without delegated credentials. Unlike retained inventory,
it omits unavailable payloads. See [memory evidence graphs](../api/memory-evidence-graph.md).
