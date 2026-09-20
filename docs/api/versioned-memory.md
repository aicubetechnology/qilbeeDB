# Versioned Memory API

The platform memory API stores records, version indexes and idempotency receipts
in the existing RocksDB memory backend at `<data-directory>/agent-memory`.
Mutations acknowledge one atomic batch with WAL and synchronous writes. There is
no implicit episode-retention quota or required model-provider service.

## Base URL and authentication

```text
http://localhost:7474/api/v1/memory
```

Use `Authorization: Bearer <platform-credential>`. Provision credentials through
the [platform API](platform-http.md). Every command, read and query authenticates
and checks the exact resource grant. The tenant comes from the credential.

A scope contains `project_id`, nullable `mission_id`, `agent_id` and `visibility`
(`private` or `shared`). Missing/null mission means the missionless scope, never a
wildcard. Private memory belongs to the authenticated subject. Shared memory is
available to other subjects only with the same exact grant inside that tenant.
No JSON field can override tenant, record author, revision or storage namespace.

## Endpoint summary

| Method and path | Capability | Success |
| --- | --- | --- |
| `POST /commands` | `memory_write` | 200 with durable receipt, including on replay |
| `GET /records/{record_id}` | `memory_read` | 200 with current record and scope |
| `POST /query` | `memory_read` | 200 with filtered records and continuation |

Create, update and delete share the command endpoint so the same receipt protocol
covers every mutation. Reads and queries return current state and do not persist
idempotency receipts or reserve a historical snapshot.

## Create a record

```bash
curl --request POST 'http://localhost:7474/api/v1/memory/commands' \
  --header "Authorization: Bearer $QILBEE_TOKEN" \
  --header 'Content-Type: application/json' \
  --data '{
    "contract_version": 1,
    "idempotency_key": "session-42-observation-1",
    "scope": {
      "project_id": "research", "mission_id": "mission-42",
      "agent_id": "researcher", "visibility": "shared"
    },
    "operation": {
      "type": "create",
      "record": {
        "episode_type": "Observation",
        "event_time_millis": 1700000000000,
        "valid_until_millis": null,
        "content": {
          "primary": "The tool returned three matching documents.",
          "secondary": null,
          "context": "Literature review",
          "data": {"document_ids": ["paper-a", "paper-b", "paper-c"]},
          "embedding": null
        },
        "tags": ["literature", "tool-output"],
        "metadata": {"source_request_id": "tool-call-17"}
      }
    }
  }'
```

Record fields:

| Field | Type | Contract |
| --- | --- | --- |
| `episode_type` | Enum | `Conversation`, `TaskExecution`, `Observation`, `Decision`, `Error`, or `{"Custom":"name"}`; case-sensitive existing Rust episode types |
| `content.primary` | String | Required text; stored without assigning truth or verification status |
| `content.secondary` | String/null | Optional second text, such as a response |
| `content.context` | String/null | Optional context text |
| `content.data` | JSON/null | Structured user data; remains untrusted content |
| `content.embedding` | Number array/null | Optional finite values retained as data; this endpoint does not build or validate a semantic index |
| `event_time_millis` | Signed integer | Required event timestamp in Unix milliseconds; unsupported date range is rejected |
| `valid_until_millis` | Signed integer/null | Exclusive visibility expiry; null means no configured expiry |
| `tags` | String array | Defaults to an empty list; exact tag filtering |
| `metadata` | JSON object | Defaults to an empty object; caller annotations, not authentication or verification authority |

Unknown contract fields are rejected rather than silently discarded. Optional
content fields may be omitted or null. Payloads use JSON; no tokenizer or implicit
token count is involved.

Illustrative response:

```json
{
  "contract_version": 1,
  "receipt": {
    "contract_version": 1,
    "idempotency_key": "session-42-observation-1",
    "record_id": "15d5c731-4e35-443c-9309-06a6561314e9",
    "revision": 1,
    "action": "created",
    "committed_at_millis": 1700000000123,
    "author": {
      "credential_id": "c096714c-4c0b-4b99-900b-8b9aad818d0d",
      "subject_id": "research-agent-user"
    }
  }
}
```

The server generates record IDs and captures the credential and subject that
performed the mutation. This is attribution, not evidence that content is true.

## Idempotency and uncertain responses

An idempotency key is scoped to the authenticated tenant, resource namespace and
subject. It contains 1–256 UTF-8 bytes, must not be blank, and cannot contain
control characters. Two credentials for the same subject and scope can recover
the same receipt if both currently have write authority.

Retry the same normalized command with the **same key** after a timeout or lost
response. The original receipt, timestamp, author, ID and revision are returned
without another mutation. Field order and equivalent omitted/null optional
fields do not change the normalized command. Changed effective payload under the
same key returns `409 idempotency_conflict`.

Receipts survive updates, deletion and restart. Replaying an old create after
deleting its record returns the original create receipt and **does not recreate
content**. A receipt proves the command committed then; read the record separately
for its current state. Invalid requests and revision conflicts do not consume a
key. Receipts have no automatic retention policy in this increment.

## Read a current record

```bash
curl --get 'http://localhost:7474/api/v1/memory/records/15d5c731-4e35-443c-9309-06a6561314e9' \
  --header "Authorization: Bearer $QILBEE_TOKEN" \
  --data-urlencode 'contract_version=1' \
  --data-urlencode 'project_id=research' \
  --data-urlencode 'mission_id=mission-42' \
  --data-urlencode 'agent_id=researcher' \
  --data-urlencode 'visibility=shared'
```

For a missionless scope, omit `mission_id`; do not send the literal string `null`.
The response contains `contract_version`, `scope`, and `record`. A record contains
`schema_version`, `record_id`, `revision`, `created_at_millis`,
`modified_at_millis`, `author` for the last mutation, and `payload` with the record
fields above. Creation time stays fixed across updates. Expired, deleted, absent
or differently scoped IDs return 404. A malformed UUID returns 400.

## Update with an expected revision

Send a new key and a complete replacement payload:

```json
{
  "contract_version": 1,
  "idempotency_key": "session-42-observation-1-correction",
  "scope": {"project_id":"research","mission_id":"mission-42","agent_id":"researcher","visibility":"shared"},
  "operation": {
    "type": "update",
    "record_id": "15d5c731-4e35-443c-9309-06a6561314e9",
    "expected_revision": 1,
    "record": {
      "episode_type": "Observation",
      "event_time_millis": 1700000000000,
      "content": {"primary":"Two of the documents meet the criteria."},
      "tags": ["corrected"]
    }
  }
}
```

A successful update returns action `updated` and revision 2. It replaces the
payload; omitted optional values reset to defaults rather than acting as a patch.
Only one concurrent write against the same revision can succeed. Other writers
receive `409 revision_conflict` and must read current state before deciding on a
new command. An explicit authorized update can renew an expired record's validity.
Historical payload versions and transitive source invalidation are separate work.

## Delete a record

```json
{
  "contract_version": 1,
  "idempotency_key": "session-42-observation-1-delete",
  "scope": {"project_id":"research","mission_id":"mission-42","agent_id":"researcher","visibility":"shared"},
  "operation": {
    "type": "delete",
    "record_id": "15d5c731-4e35-443c-9309-06a6561314e9",
    "expected_revision": 2
  }
}
```

Success returns action `deleted` and revision 3. The current payload is replaced
with a tombstone, preserving revision and receipt identity. Ordinary reads and
queries no longer return it. A new command cannot update a tombstone. This is
logical content deletion, not guaranteed physical erasure from historical WAL,
SST files or backups. Receipt digests do not contain the original payload text.

## Query and pagination

```json
{
  "contract_version": 1,
  "scope": {"project_id":"research","mission_id":"mission-42","agent_id":"researcher","visibility":"shared"},
  "filter": {
    "limit": 20,
    "after": null,
    "text_contains": "documents",
    "episode_type": "Observation",
    "tag": "literature"
  }
}
```

`POST /query` returns `page.records`, `page.next_after` and
`page.scanned_records`. Text filtering performs case-insensitive substring
matching independently against primary, secondary and context text. It does not
search structured JSON, emit synthetic similarity scores, or claim indexed BM25
or semantic retrieval. Type and tag filters are exact. Deleted/expired records
are excluded, and record/index versions are checked before using each record.

Results are ordered by UUID bytes. Pass `next_after` as the next request's `after`
until it is null. A continuation may yield an empty final page. Each request
returns at most 1,000 records and scans at most `filter.scan_limit` entries
(1–10,000, default 10,000); continuation
exposes remaining work instead of silently truncating the traversal. This is a
technical work bound, not a stored-memory quota. Pagination is a live traversal:
concurrent inserts before the cursor are not a snapshot or a durable changes feed.

## Errors, consistency and limits

The [platform error envelope](platform-http.md#errors-and-transport-limits)
applies. Memory-specific codes include `record_not_found` (404),
`idempotency_conflict` (409), `revision_conflict` (409), and
`storage_inconsistency` (500). Unsupported stored versions, missing point-lookup
counterparts or mismatched record/index revision/digest fail explicitly.

JSON request bodies are limited to **65,536 bytes**, not tokens. All memory I/O
runs in blocking workers. Credential checks apply to each request; a request
already authorized may finish while a credential is revoked concurrently.
The storage engine serializes these local memory operations; this is not a
multi-node consensus guarantee.

Platform records use separate versioned key prefixes inside the same existing
memory column families. Legacy global agent records are not assigned to tenants
automatically. Both formats remain readable by their respective explicit APIs.

## Validation

The workspace suite tests receipts after reopen, changed-payload rejection,
concurrent retries, conditional update conflicts, deletion replay, scope and
capability boundaries, private/shared subjects, expiry, filtering, pagination and
injected record/index version mismatch. A real HTTP child process acknowledges
20 commands, is killed without graceful shutdown, then recovers every record and
returns every original receipt on retry with an exact total of 20 records.

This validates process-crash recovery on the test filesystem. Hardware power
loss, backup/restore, indexed retrieval benchmarks, procedural HTTP and shared
change feeds remain separate acceptance criteria.

## Model-bound semantic retrieval

The text query above remains a substring/filter endpoint. Use the separate
[semantic search API](semantic-memory.md) to attach externally generated vectors
to exact source revisions and retrieve by cosine similarity. It enforces model
identity and scope and discloses partial scan coverage.

## Review decisions

[Memory review](memory-review.md) adds optional review metadata to records.
Rejected revisions are unavailable to ordinary reads, queries and retrieval.
Content updates clear the current review; historical decisions remain durable.
Every decision advances the revision and requires new revision-bound embeddings.

## Derived content

The `derive` operation requires both read and write capabilities and creates a
record bound to exact source revisions. [Derived memories](derived-memory.md)
validate source eligibility before retrieval and cannot be edited with ordinary
updates. Query pages include additional dependency-work counters.

`filter.limit` bounds returned records; `filter.scan_limit` bounds examined records,
including unavailable ones. To reduce dependency-validation work during initial
consumer reconciliation, use a smaller scan limit. An empty page may still have
`next_after`; continue until it is null. A single oversized dependency can still
exceed the separate validation budget and fail explicitly.
