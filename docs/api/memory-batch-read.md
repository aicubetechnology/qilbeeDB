# Read a context in one memory snapshot

`POST /api/v1/memory/records/batch` reads **1–100 distinct memory IDs** in one
authorized scope. All requested records and their transitive sources use the
same storage snapshot and server evaluation time. This feature is available in
the batch-read feature build; discover the route in the server's `/openapi.json`
before using it. Earlier 0.10.0 images do not provide it.

Use this endpoint when constructing or revalidating a context made from several
memories. It replaces one HTTP read per memory with one request for the group.
It does not search, generate embeddings, create a cache, or advance consumer
progress. Ranking versions and existing individual reads remain unchanged.

## Request current records

Use a current bearer credential with `memory_read` and an exact resource grant.
Tenant and the subject of private memory come from the credential. A batch cannot
mix projects, missions, agents, tenants, or private subjects.

```bash
curl --fail-with-body 'http://localhost:7474/api/v1/memory/records/batch' \
  --header "Authorization: Bearer $QILBEE_TOKEN" \
  --header 'Content-Type: application/json' \
  --data '{
    "contract_version": 1,
    "scope": {
      "project_id": "research",
      "mission_id": "mission-42",
      "agent_id": "researcher",
      "visibility": "private"
    },
    "record_ids": [
      "15d5c731-4e35-443c-9309-06a6561314e9",
      "31cb94eb-9233-41cf-82f1-ea42dfc7d260"
    ]
  }'
```

Set `mission_id` to `null` for missionless memory. IDs are parsed as UUIDs;
different textual spellings of the same UUID still count as duplicates. Unknown
request fields, duplicate IDs, an empty list, or more than 100 IDs are rejected.

## Interpret every entry

HTTP 200 contains `contract_version`, the requested `scope`, and `batch`:

| Field | Meaning |
| --- | --- |
| `evaluated_at_millis` | One server timestamp, in Unix milliseconds, used for eligibility throughout the batch |
| `entries` | Exactly one entry per requested UUID, preserving request order |
| `entries[].record_id` | The requested UUID, including when unavailable |
| `entries[].record` | The complete current `MemoryRecord`, or `null` |
| `record_bytes` | Serialized root-record bytes examined, including unavailable roots; excludes indexes and dependency reads |
| `dependency_work` | Aggregate distinct dependency reads and serialized bytes across this request |

A non-null record has the same ID as its entry and satisfies the same eligibility
rules as [an individual current read](versioned-memory.md#read-a-current-record).
Deleted, expired, rejected, missing, or otherwise ineligible records return `null`.
Invalid transitive dependencies also make a record unavailable. A UUID from
another authorized namespace is never looked up outside the requested namespace.
The response gives no per-record unavailability reason; work counters describe
aggregate work, including ineligible roots. Review explanations still require
their separate authorization.

For example, two unavailable IDs produce two entries, not an empty success list:

```json
{
  "contract_version": 1,
  "scope": {
    "project_id": "research",
    "mission_id": "mission-42",
    "agent_id": "researcher",
    "visibility": "private"
  },
  "batch": {
    "evaluated_at_millis": 1700000000000,
    "entries": [
      {"record_id": "15d5c731-4e35-443c-9309-06a6561314e9", "record": null},
      {"record_id": "31cb94eb-9233-41cf-82f1-ea42dfc7d260", "record": null}
    ],
    "record_bytes": 0,
    "dependency_work": {"records_examined": 0, "bytes_examined": 0}
  }
}
```

The zero byte counts in this example describe absent IDs. Stored tombstones or
ineligible records can incur work even when all entries are null.

## Revalidate a retained context

1. Bring the application's invalidation consumer to its fixed catch-up fence.
2. Read the context's exact IDs using this endpoint. Verify the returned scope,
   entry count, order and IDs before using any content.
3. Discard the retained context if a required entry is null or its revision
   differs from the revision used to construct that context. Compare revisions
   as integers without floating-point conversion.
4. Apply the application's existing before/after feed synchronization and
   invalidation-epoch checks. A batch does not commit or acknowledge a checkpoint.
5. If transport, authentication, contract validation or server evaluation fails,
   treat the context as **unvalidated**. Preserve durable consumer progress.

Expiry can occur without any change-feed event, including in an ancestor whose
own expiry is not copied into the derived record. Reaching the feed watermark
therefore does not replace current eligibility checks. Historical checkpoint
receipts also do not replace reading current checkpoint state.

`evaluated_at_millis` is an observation, **not a validity lease**. It is not a
change cursor, cannot be passed to a checkpoint, and does not prevent subsequent
mutations, revocation, or expiry. The endpoint does not promise that the context
remains valid throughout an agent task or authorize an external action. Multiple
batches are separate snapshots; splitting more than 100 IDs does not produce an
atomic context across the requests.

## Limits and failure handling

| Limit | Behavior |
| --- | --- |
| 1–100 distinct IDs | Fixed contract limit; no pagination or silent truncation |
| 64 KiB request body | HTTP 413 if the JSON transport exceeds the limit |
| 8 MiB root-record bytes | HTTP 400 if exceeded, including stored unavailable roots |
| 4,096 distinct dependency lookups / 16 MiB dependency bytes | Shared across all roots; HTTP 400 on exhaustion |
| Existing derivation graph bounds | An ineligible graph yields a null record, as with individual reads |
| Shared retrieval execution slots | HTTP 503 `retrieval_busy` if no slot is available after authorization |

The 8 MiB root budget is fixed and independent of lexical/hybrid scan settings.
Neither byte budget is an RSS or exact response-size limit: indexes, wrappers,
temporary objects and the already-fetched value that crosses a budget are
additional work. Dependencies shared between roots reuse one request-local
validated record cache. It never persists across snapshots or scopes.

Admission shares `QILBEE_MAX_CONCURRENT_RETRIEVALS` with lexical, cosine and hybrid
search. The permit covers the blocking read and response serialization, and stays
held if the caller disconnects while the operation is running. Single-record
GETs retain their existing behavior. Configure client deadlines, bounded backoff
for 503, and appropriate container limits.

The server returns **no successful prefix** when a budget, dependency read or
integrity check fails. HTTP 500 signals an encountered internal/integrity failure;
it must not be interpreted as an unavailable record. HTTP 401/403 apply to the
whole request. Successful responses and errors use `Cache-Control: no-store`.

See the [context-consumer qualification](../research/context-consumer-qualification.md)
for the integration evidence motivating this contract and its remaining limits.
