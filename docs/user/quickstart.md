# Create and retrieve your first memory

This guide creates a durable memory and retrieves it by text. Embeddings are
optional for this workflow. You can add semantic or hybrid retrieval afterward
using your own embedding service.

## Before you begin

Run the [local Docker service](deployment.md), or obtain the base URL of a running
QilbeeDB deployment. An operator must issue a platform credential with
`memory_write` and `memory_read`, granting this exact scope:

```json
{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"}
```

An administrator credential does not automatically include memory permissions.
See [credential issuance](../api/platform-http.md) to configure an application
credential. Examples below use Bash, `curl`, and the `QILBEE_TOKEN` environment
variable containing that application's credential. Requests to a remote service
should use its HTTPS URL.

## 1. Check the service

```bash
curl --fail --silent --show-error http://localhost:7474/health
```

Check the returned server version. These lexical and hybrid endpoints are part
of the 0.5.0 release; 0.4.0 supports the separate semantic endpoint.

## 2. Create a memory

```bash
curl --fail --silent --show-error \
  http://localhost:7474/api/v1/memory/commands \
  --header "Authorization: Bearer $QILBEE_TOKEN" \
  --header 'Content-Type: application/json' \
  --data '{
    "contract_version": 1,
    "idempotency_key": "quickstart-observation-v1",
    "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
    "operation": {
      "type": "create",
      "record": {
        "episode_type": "Observation",
        "event_time_millis": 1700000000000,
        "content": {"primary":"Error ZX17 means the retry budget was exhausted."},
        "tags": ["runbook"],
        "metadata": {"source":"quickstart"}
      }
    }
  }'
```

Save the receipt's `record_id` and `revision`. The receipt is acknowledged after
an atomic, synchronously persisted write. Retrying the identical command with
the same subject, scope and idempotency key returns the original receipt. Using
that key for a different command returns 409. Use a new key for a new operation.

## 3. Retrieve it by text

```bash
curl --fail --silent --show-error \
  http://localhost:7474/api/v1/memory/search/lexical \
  --header "Authorization: Bearer $QILBEE_TOKEN" \
  --header 'Content-Type: application/json' \
  --data '{
    "contract_version": 1,
    "mode": "lexical",
    "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
    "query": {"text":"ZX17","limit":10,"tag":"runbook"}
  }'
```

Find your source record in `page.hits`. Its `score` is BM25, and
`ranking_version` identifies `bm25_v1`. Check `page.exhaustive` before treating
the result as a ranking over the entire scope. A larger corpus may exceed a
request's scan budget even if fewer than ten hits were returned.

## 4. Add external embeddings when needed

Read the current source revision, generate its embedding outside QilbeeDB, and
attach the vector with provider, model, immutable model revision and dimensions.
Use the same embedding configuration for queries. Never assume vectors from
different spaces are comparable merely because their lengths match.

Follow [semantic search](../api/semantic-memory.md) for the attachment and cosine
contracts. Then evaluate [hybrid search](../api/hybrid-memory.md) with explicit
text, vector and `weighted_rrf_v1`. The hybrid profile is experimental; compare
it on held-out queries before selecting it for your application.

## Update or delete safely

Updates and deletions require the current `expected_revision` and a new
idempotency key. A successful update changes the source revision and immediately
makes its old vectors ineligible in subsequent queries. Lexical retrieval reads
the new text immediately. Attach a new vector for the updated revision when it
is available. Deletion and expiry exclude the record from both retrieval paths.
See [memory lifecycle and receipts](../api/versioned-memory.md).
