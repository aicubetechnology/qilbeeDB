# Read memories in creation order with Python

Use `OrderedMemoryClient` to browse current memories in one authorized scope, newest or oldest first. The server orders the complete selection before pagination. The client uses only the Python standard library and does not generate embeddings, register an agent, create a checkpoint or retry requests automatically.

This additive SDK feature requires a server that implements `POST /api/v1/memory/query/ordered`. It is unreleased. For the managed platform, verify availability before enabling it. For self-hosted installations, deploy a compatible server first. An unsupported endpoint is an error; the SDK never substitutes the older UUID listing or sorts an incomplete page locally.

## Read a page

Set the URL and credentials in your application environment. Supply the expected company and credential subject explicitly; these are checked against the server identity on every operation. The credential needs `memory_read` for the selected scope.

```python
import os
from qilbeedb import OrderedMemoryClient

reader = OrderedMemoryClient(
    os.environ["QILBEEDB_URL"],
    lambda: os.environ["QILBEEDB_API_KEY"],
    tenant_id=os.environ["QILBEEDB_TENANT_ID"],
    subject_id=os.environ["QILBEEDB_SUBJECT_ID"],
    scope={
        "project_id": "support",
        "agent_id": "assistant",
        "mission_id": None,
        "visibility": "private",
    },
)
response = reader.query(order="created_desc", limit=40, scan_limit=1000)
page = response["page"]
for record in page["records"]:
    print(record["created_at_millis"], record["payload"]["content"]["primary"])
print(page["stop_reason"], page["next_cursor"])
```

Private access uses the authenticated subject. Shared access still requires an explicit grant. This scoped client does not confer company-wide administrative visibility.

## Continue or change the selection

Pass `page["next_cursor"]` into the next `query` call with the same order and filters. Only `stop_reason == "exhausted"` with a null cursor means traversal finished. A page can contain no records while still returning a cursor: filters, current eligibility and work budgets can leave more positions to examine. Applications should bound the number of requests they make.

Change the order to `created_asc` for oldest first. Optional filters are `text_contains`, `tag` and `episode_type`. Changing the order, scope or filters requires starting again with `cursor=None`; the server rejects an incompatible cursor. Limits may change between requests. The result limit is 1–100, and the scan limit is 1–10,000 per request. The server also applies byte and dependency-work limits.

The response retains `scanned_records`, `record_bytes`, `dependency_work` and `evaluated_at_millis`. Do not interpret a partial page as an empty scope or conceal its coverage limit.

Creation time is immutable and assigned by the server; editing a memory does not move it to the newest position. UUID breaks timestamp ties within a scope. Pages are live observations, not a snapshot. A new record before the continuation position requires a refresh to see it. Reads recheck current eligibility, including deletion, rejection, expiry and dependencies. Re-read a memory before consequential reuse; a prior page is not an authorization reservation.

## Handle errors without losing position

The client reuses the SDK's `ConsumerAPIError`, `ConsumerProtocolError`, `ConsumerTransportError` and `ReconciliationRequired` exceptions. Their `checkpoint_outcome` is `not_attempted`: this reader does not write checkpoints. Preserve your last successful page and cursor when a request fails, while clearly marking those records as an earlier observation.

- For HTTP 400 due to a changed selection or invalid cursor, start a new traversal.
- For HTTP 401 or 403, refresh credentials or resolve the grant; do not reuse cached results as newly authorized.
- A credential for a different expected company or subject raises `ReconciliationRequired` before querying memory. Construct a reader with the deliberately selected identity rather than silently switching context.
- Transport errors and HTTP 503 are surfaced without retry. The application may retry the same read after recovery.
- Protocol validation checks the returned scope, page bounds, ordering, duplicate IDs and continuation consistency. It does not independently prove the server's entire index or cryptographically authenticate an opaque cursor.

The default response cap is 16 MiB and the socket timeout is 10 seconds; both are configurable. The transport rejects redirects and does not use environment proxies or persist credentials. Concurrent use of the same reader is rejected; applications may create independent readers for separately managed requests.
