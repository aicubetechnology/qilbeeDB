# Configure retrieval capacity

QilbeeDB accepts externally generated vectors with **1 through 32,768 dimensions**.
This includes 1,536, **3,072**, 4,096 and 8,192 dimensions. Declare the provider,
model, revision and exact dimensions for every attachment and query. Each identity
is a separate embedding space: vectors are never truncated, padded, projected or
compared across spaces. A source can have current bindings in several spaces.
Embedding generation and provider credentials remain outside the database.

## Set operator limits

Configure these environment variables before starting the server. Invalid values
fail startup. The values apply to one server instance, not a tenant entitlement,
ranking version or relevance threshold.

| Variable | Default | Accepted range | Behavior |
| --- | --- | --- | --- |
| `QILBEE_MAX_EMBEDDING_DIMENSIONS` | `32768` | `1`–`32768` | Maximum dimensions accepted by vector attachment and query endpoints |
| `QILBEE_MAX_RETRIEVAL_SCAN_BYTES` | `67108864` (64 MiB) | `8388608`–`268435456` (8–256 MiB) | Ceiling for lexical/hybrid `scan_bytes_limit` |
| `QILBEE_MAX_CONCURRENT_RETRIEVALS` | `2` | `1`–`64` | Shared concurrent execution slots for lexical, cosine, hybrid retrieval and memory batch reads |

Changing a ceiling does not change the request defaults. Lexical and hybrid
requests still default to an 8 MiB scan budget and 10,000 scanned sources. A client
must explicitly request a larger byte budget. The result limit remains 1–100;
the existing fusion profile retains its 100-candidate cap per channel.

For example, configure 128 MiB scans and two concurrent retrievals with Compose:

```bash
QILBEE_MAX_EMBEDDING_DIMENSIONS=32768 \
QILBEE_MAX_RETRIEVAL_SCAN_BYTES=134217728 \
QILBEE_MAX_CONCURRENT_RETRIEVALS=2 \
docker compose up -d
```

Persist these operator settings in the deployment configuration so recreation does
not revert them. Lowering the dimension ceiling leaves persisted bindings intact,
but queries and new attachments above that ceiling are rejected. Restore a
sufficient ceiling to access those spaces through vector endpoints.

## Discover the active limits

Call `GET /api/v1/memory/ranking-profiles` with a current credential that grants
`memory_read`. The `execution_limits` object accompanies the immutable ranking
profiles:

```json
{
  "scope": "server_instance",
  "max_embedding_dimensions": 32768,
  "max_scan_bytes": 134217728,
  "default_scan_bytes": 8388608,
  "max_concurrent_retrievals": 2,
  "vector_request_body_bytes": 2097152
}
```

These limits disclose configuration only. They do not disclose corpus sizes,
private records, available slots or tenant statistics. Authentication and exact
scope authorization precede retrieval admission and candidate selection.

## Size requests and scans separately

`POST /api/v1/memory/embeddings`, `/api/v1/memory/search`,
`/api/v1/memory/search/hybrid` and `/api/v1/memory/search/graph` accept JSON bodies
up to **2 MiB**. This includes
scope, text, model identity, the vector, numeric formatting and whitespace. Other
memory endpoints retain their 64 KiB body limit. Login and login-account JSON
requests have a separate 8 KiB limit. A vector must also contain only
finite float32 values, match its declared dimension exactly and have nonzero norm.

The four vector-capable OpenAPI operations expose the byte limit as
`x-qilbee-max-request-body-bytes`; their HTTP 413 descriptions use the same value.
This is a whole-body transport limit, not a JSON Schema string-length constraint
or an embedding dimension allowance. A smaller valid query may still fail another
limit. Documentation extensions describe the published router and do not change
operator configuration or request processing.

A 3,072-dimensional float32 vector has 12,288 raw component bytes; an 8,192-dimensional
vector has 32,768; a 32,768-dimensional vector has 131,072. JSON on the wire and in
storage occupies more space. The request body limit and the retrieval scan budget
are independent. Discovering that a dimension is supported does not guarantee a
particular corpus fits a particular scan budget.

For lexical/hybrid retrieval, `scanned_bytes` counts serialized source records
and, for hybrid, selected-space embedding bindings examined under the authorized
scope. It excludes index keys, allocator overhead, temporary objects and lookahead;
**it is not a process memory limit**. Increasing dimensions or corpus size raises
CPU and storage costs. Measure peak RSS, latency and concurrency on the deployment
hardware, and use container resource limits appropriate to that deployment.

## Require complete coverage when comparing rankings

To rank a larger corpus in one request, explicitly set, for example,
`"scan_bytes_limit": 134217728`. Verify all of the following:

- `exhaustive` is `true`, with no `next_after` cursor.
- `corpus_records` matches the frozen authorized corpus.
- Hybrid `embedded_records` matches the corpus and `embedding_coverage` is `complete`.
- The expected source revisions, embedding identity and ranking profile match.
- Inspect `candidates_truncated` independently: complete scanning still permits
  the published per-channel candidate cap.

Every request uses one coherent storage snapshot. A cursor continues a source scan
in a later request; it does not preserve that snapshot. Do not concatenate or fuse
BM25 page scores to claim a complete global ranking, because corpus statistics
would differ. The legacy cosine response and cosine score retain their existing
contract; its source coverage uses the existing embedding-count scan limit.

## Handle resource errors

| HTTP status | Error code | Client action |
| --- | --- | --- |
| `400` | `embedding_dimension_limit` | Check the declared dimension and operator ceiling; do not resize vectors implicitly |
| `400` | `retrieval_scan_limit` | Request an allowed scan budget or have the operator adjust capacity |
| `413` | `invalid_request` | Reduce JSON transport size while preserving the complete vector |
| `503` | `retrieval_busy` | Retry with bounded backoff and jitter within the caller's deadline |

Admission is nonblocking: no retrieval starts when its execution slot is unavailable.
The [memory batch-read route](../api/memory-batch-read.md) uses the same admission
pool and returns the same `retrieval_busy` error. Its fixed 8 MiB root-record
budget is independent of `QILBEE_MAX_RETRIEVAL_SCAN_BYTES`.
The slot remains held until the blocking retrieval finishes, even if the HTTP client
disconnects, and is released on success or failure. This bounds simultaneous retrieval
work; it is not a global HTTP rate limiter or a per-tenant fairness scheduler.

## Evaluate dimensionality as a model choice

More dimensions provide more representational capacity for models designed to use
it, but dimensionality alone is not evidence of better relevance. Compare supported
model configurations using frozen vectors, the same corpus and judgments, and
separate development/test queries. Record storage, scan coverage, latency and
external generation cost as well as relevance. Synthetic large-vector tests establish
contract behavior, persistence and isolation; they do not establish semantic quality.

For research on explicitly trained representations at multiple granularities, see
[Matryoshka Representation Learning](https://arxiv.org/abs/2205.13147). Such model
capabilities belong to the external encoder contract. QilbeeDB does not infer that
a particular model supports truncation and does not implement a Matryoshka cascade
or an approximate vector index in this release.

## Error-schema compatibility

The OpenAPI error-code catalog includes `embedding_dimension_limit`,
`retrieval_scan_limit` and `retrieval_busy`. The search 503 response schemas are
specific to `retrieval_busy`. A successful partial result still uses 200 with
coverage fields; it is different from an invalid budget or rejected admission.
Real HTTP contract tests hold a slot deterministically to exercise overload,
release it to verify recovery, and check authorization precedence. They do not
rely on a load race or change process-wide environment variables.
