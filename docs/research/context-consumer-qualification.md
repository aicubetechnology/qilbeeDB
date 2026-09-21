# Context consumer qualification and batch-read acceptance

The Qilbee integration team qualified durable context invalidation against a
real QilbeeDB 0.10.0 HTTP server and PostgreSQL 15. This is evidence about the
application's invalidation and acknowledgement boundary. It does not establish
agent-task improvement, retrieval relevance, or production rollout.

## Attributed integration evidence

The published [integration report](https://github.com/aicubetechnology/qilbee-ecosystem/blob/0244327f18d2cd50e62b775ddeb38ccc11b0fb28/docs/qilbeedb-context-consumer.md)
and [sanitized observations](https://github.com/aicubetechnology/qilbee-ecosystem/blob/0244327f18d2cd50e62b775ddeb38ccc11b0fb28/docs/validations/2026-09-20-qilbeedb010-context-consumer.json)
are frozen at integration commit `0244327f18d2cd50e62b775ddeb38ccc11b0fb28`.
The observations file has SHA-256
`2f7fe2edc7c07f0db4d1dff4c68bd2b53f5d32f92e33a380ee7dbe3028bc8ca9`.

The tested database image was
`sha256:95bc2a74aab0c4369efbc7274b74409b023fef63d52845c4b7f86911c0230afa`,
at database revision `59bc4d502ea4c7b8d4a413cb084a23d21841896b`.
This was an earlier 0.10.0 feature image, not the subsequently deployed merged
main image. The test used one tenant, one scope and seven synthetic records,
with no model or embedding calls.

| Reported observation | Boundary established by that integration test |
| --- | --- |
| Source rejection invalidated second-level derived context after consumer restart | Live eligibility and retained invalidation state both mattered |
| Clock expiry left the feed cursor unchanged but prevented context reuse | Feed catch-up alone was insufficient |
| Consumer SIGKILL after local SQL commit, before remote acknowledgement | Pending exact acknowledgement resumed without reapplying invalidation |
| Successful checkpoint response was dropped | Reusing the persisted command recovered its original receipt |
| Six consumers raced on the same SQL state | One applied invalidation; five received local update conflicts |
| PostgreSQL SIGKILL and restart | The observed epoch, cursor and receipt survived |
| Another writer advanced remote progress | The consumer stopped on divergence without fast-forwarding local effects |

The report records 292 automated tests: 221 client, 32 consumer, 35 embedding
harness and four history qualification tests. It reports zero failures and 41
pre-existing deprecation warnings. QilbeeDB's evidence review verified that these
counts sum correctly and that all seven supplied source-file digests match the
frozen integration commit. It did **not** independently replay the PostgreSQL
campaign or inspect unsanitized runtime artifacts.

The invalidation library was not connected to agents, the terminal, webchat or
the production API. There was no migration of real memories. Six competing
consumers are not six agents completing tasks, and SQL invalidation does not
prove exactly-once external tool execution.

## Implications for the database contract

The integration correctly treats a historical checkpoint receipt as evidence of
its original command and compares current checkpoint state separately. Preserve
that behavior when using [consumer diagnostics](../api/consumer-diagnostics.md)
or the [lightweight SDK](../api/verified-memory-consumer.md).

The integration's individual record reads produced no atomic multi-record view
and incurred one request per reused memory. The
[batch-read endpoint](../api/memory-batch-read.md) addresses that specific gap:
one authorized scope, one coherent record/dependency snapshot, one eligibility
clock and one ordered entry per requested ID. It remains a point-in-time
observation; the clock and new mutations can invalidate memory afterward.

Qualification must preserve expiration without a mutation event, transitive
rejection, revision changes, corruption failures, restart, exact request order,
aggregate work limits, and tenant/private-subject isolation. Real HTTP responses
must conform to the OpenAPI served by the tested image. Compare per-record reads
and batches on unchanged IDs and revisions; report network conditions, number of
requests, latency and returned bytes separately from retrieval quality.

## Reproduce the context-read comparison

`scripts/benchmark_context_reads.py` is a read-only comparison harness. Provide a
JSON file containing one `scope` object and a `record_ids` array of 100 distinct,
currently eligible IDs. Use the scope fields from the batch-read request. Hold
the corpus unchanged for the run; the harness freezes full records on its first
read and aborts if any later observed content, revision or eligibility changes.

```bash
python3 scripts/benchmark_context_reads.py \
  --base http://localhost:7474 \
  --fixture /path/to/authorized-context.json \
  --token-env QILBEE_TOKEN \
  --sizes 1 10 50 100 --samples 20 \
  --output /path/to/new-context-read-report.json
```

The output path must not already exist. The tool reads the bearer token from the
named environment variable and does not persist it or record bodies. It makes no
mutations or provider calls and does not retry failed requests. A failed run can
leave an empty output file; only a valid report with `complete: true` establishes
completion. Preserve failures separately before choosing a new output path.

The harness uses one HTTP/1.1 connection, two warmups per mode/size and alternating
mode order for the measured pairs. Latency includes HTTP transfer and client JSON
decoding; equality checks are outside the timer. It reports every sample, median,
nearest-rank p95, request count and response-body bytes. Record the tested image,
machine and transport conditions alongside the report. It does not measure server
CPU, RSS, cold caches or concurrent capacity, and does not simulate agent reasoning.

Integration with the agent's actual context path remains a separate acceptance
gate. A long-running task must discard a source corrected during execution and
resume after restart using this consumer. Keep the model, prompt, tools and task
criteria fixed before attributing any task improvement to memory.
