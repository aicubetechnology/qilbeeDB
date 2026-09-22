# Discover learning policies and evaluation contexts

**Availability: source preview.** This endpoint is being qualified and is not
part of the deployed API. Check the running server's OpenAPI before using it.

Use `POST /api/v1/learning/metadata/query` to find policies and evaluation contexts
without knowing their IDs. The summaries include immutable registry versions and
digests. Discovery does not qualify a policy, select a procedure or authorize an
agent to execute anything.

## Access

Every page requires a current company credential with `learning_metadata_read`
or `policy_admin`. Company identity comes from authentication. No project, agent,
mission, private subject or memory grant is required or created. The separate
company administrative inventory retains its existing permissions;
`credential_admin` alone does not authorize this new endpoint.

The company or agent application supplies policy content and evaluation meaning.
QilbeeDB stores, validates and retrieves the registered records. Embeddings and
model-provider credentials are not involved in metadata discovery.

## Find a record

```json
{
  "contract_version": 1,
  "query": {
    "kind": "context",
    "text": "incident review",
    "limit": 25,
    "max_scanned_records": 100,
    "cursor": null
  }
}
```

Only `contract_version` and `query.kind` are required. Kind is `policy` or
`context`; other resource kinds and unknown properties are rejected. Text is an
optional case-insensitive substring of the displayed title or ID, not semantic
search. Omitted or null text means no filter; a supplied value must contain
1–256 UTF-8 bytes without control characters.

A successful response contains `contract_version`, authenticated `company_id`
and `page`. Each entry in `page.entries` contains:

| Field | Meaning |
| --- | --- |
| `kind` | Requested policy or context kind |
| `id` | Immutable registry ID, at most 512 UTF-8 bytes |
| `title` | Whitespace-normalized policy ID or context task, up to 160 Unicode characters |
| `schema_version` | Stored registry schema version |
| `payload_digest` | Existing SHA-256 registry payload digest; not a new client JSON-hashing convention |
| `recorded_at_millis` | Original registration time, in Unix milliseconds |

Titles need not be unique. Select the returned ID and use the existing
`GET /api/v1/learning/policies/{id}` or
`GET /api/v1/learning/contexts/{id}` endpoint to read the full record. Those reads
recheck current authority. Summaries do not include full payloads or the writer's
identity. A stored digest verifies a binding; it does not prove the truth of an
evaluation or the usefulness of a policy.

## Continue discovery without losing progress

Traversal uses deterministic registry-key byte order, not relevance or title
order. Send `page.next_cursor` unchanged as `query.cursor`. Continue even when a
page has no entries but includes a cursor. Keep kind and effective text filter
unchanged; changing them requires a fresh traversal. Page and scan limits may
change. Omitted and null text have the same binding.

Cursors bind the authenticated company, kind and filter. Treat them as opaque;
they do not grant access. Foreign-company, changed-filter, changed-kind and
malformed positions are rejected. The cursor advances after the last scanned
record, including entries excluded by the text filter.

Each page is one serialized observation. Several pages do not share a snapshot.
New records inserted before the cursor require a new traversal to discover.
`exhausted` means the remaining requested prefix was exhausted at that
observation, not that every company resource has been inventoried permanently.
An unchanged traversal advances without duplicate entries.

## Limits and coverage

| Field or limit | Meaning |
| --- | --- |
| `limit` | 1–50 returned entries; default 25 |
| `max_scanned_records` | 1–1,000 primary records; default 100 |
| Primary scan byte ceiling | 4 MiB of primary keys and values per page |
| `page.stop_reason` | `exhausted`, `entry_limit`, `scan_limit` or `byte_limit` |
| `page.next_cursor` | Null for exhaustion; continuation after a limit stop |
| `page.scanned_records` | Primary records examined, including text-filtered records |
| `page.scanned_record_bytes` | Primary key/value bytes examined |
| `page.observed_at_millis` | Start of the serialized observation |

A primary record larger than the byte ceiling fails the request rather than
returning a cursor that cannot advance. Byte counters exclude dependency reads
and database overhead; they are not measurements of CPU, total memory use or
HTTP response size. Bounded summaries and entry limits do not imply a latency
promise. Requests share retrieval admission and serialize with learning writes.

## Errors and recovery

| HTTP status | Action |
| --- | --- |
| `400` | Correct unsupported version, fields, kind, bounds, filter or cursor; restart when filters change |
| `401` | Clear protected data and authenticate again; the credential may have expired or been revoked |
| `403` | Obtain the required metadata permission; do not infer an empty inventory |
| `413` | Reduce the request below the 65,536-byte body limit |
| `503 retrieval_busy` | Retry with bounded backoff and the last accepted cursor |
| `500` | Treat storage or integrity verification as failed; do not use a partial response |

Authentication and capability checks precede retrieval admission. Responses use
`Cache-Control: no-store`. Accept a page before advancing local progress. After
a failed continuation, preserve the last accepted cursor and label any retained
rows as earlier observations; after an authorization failure, clear them.

A human interface should list available records, support selection and a return
to the list, and keep raw IDs in advanced details. A failed page, an incomplete
empty page and an exhausted search are different states. Re-read details when
opening a selection instead of treating an earlier row as current authorization.
