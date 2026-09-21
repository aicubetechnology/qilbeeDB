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

## Batch-read measurement on the feature image

The [frozen raw samples](context-read-report.json) were collected against database
commit `79da87f3731c077da1385fe45498d944e54e0d65`, image
`sha256:4b69e54d8a8dcc538c56186fa566ac8fb00ee8e805aab06561a71a731a0fdc61`.
This is a separate QilbeeDB qualification, not a rerun of the attributed
PostgreSQL campaign. All 53 tests across nine isolated Docker suites passed,
including real-clock expiry without feed changes, transitive source rejection,
abrupt server restart and the context comparison. The shared team server was
not changed.

Two synthetic corpora each contained 100 records with roughly 1 KiB of text.
The second corpus derived every record from one common source. Every measured
GET and batch returned the same complete records and revisions as the frozen
initial read; a final batch verified equality again. Feed watermarks did not
advance during the reads. No embeddings or model calls were used.

The run used local Docker over loopback, one HTTP/1.1 connection, one client,
20 measured samples per mode/size, two warmups, and alternating mode order.
Container CPU/memory settings were Docker defaults. Workstation activity was
uncontrolled. These are warm local observations, not a production capacity
guarantee or a confidence interval for p95.

| Corpus | Records | GET p50 / p95 (ms) | Batch p50 / p95 (ms) | GET / batch body bytes |
| --- | ---: | ---: | ---: | ---: |
| Independent | 1 | 0.453 / 0.616 | 0.447 / 0.833 | 1,580 / 1,771 |
| Independent | 10 | 5.404 / 8.158 | 0.834 / 1.159 | 15,800 / 15,101 |
| Independent | 50 | 28.383 / 31.927 | 1.932 / 5.304 | 80,920 / 76,261 |
| Independent | 100 | 60.168 / 76.068 | 4.052 / 8.420 | 162,320 / 152,712 |
| Shared source | 1 | 0.598 / 0.972 | 0.624 / 1.200 | 1,746 / 1,939 |
| Shared source | 10 | 7.840 / 21.571 | 1.444 / 17.724 | 17,460 / 16,763 |
| Shared source | 50 | 28.374 / 37.395 | 2.359 / 5.889 | 89,220 / 84,563 |
| Shared source | 100 | 68.536 / 80.841 | 4.755 / 6.880 | 178,920 / 169,314 |

For 100 records, requests decreased from 100 to one. The observed p95 decreased
from 76.068 to 8.420 ms for independent records, and from 80.841 to 6.880 ms
for shared-source records. The batch read one distinct shared dependency for
the whole group. **For a single record, batch p95 was higher in both corpora**;
the response wrapper also increased body bytes. The shared-source 10-record
case had substantial tail variation. Preserve these losses and raw samples
when comparing future implementations.

These measurements establish fewer HTTP round trips and observed local context
read latency with equal records. They do not establish server CPU or RSS savings,
performance under six-client load, cold-start behavior, greater retrieval
relevance, or improved agent reasoning. The correctness benefit of a batch is
one coherent point-in-time view; a series of individual GETs has separate
snapshots even when the unchanged test corpus makes their contents equal.

## Independent production integration acceptance on 0.12.0

On September 21, 2026, the Qilbee integration team published a separate
[production integration report](https://github.com/aicubetechnology/qilbee-ecosystem/blob/81eb9626516e85a55ac15a5273fad5af987717d4/docs/qilbeedb-batch-context-results.md)
and [observations](https://github.com/aicubetechnology/qilbee-ecosystem/blob/81eb9626516e85a55ac15a5273fad5af987717d4/docs/validations/2026-09-21-qilbeedb012-batch-context.json)
at immutable commit `81eb9626516e85a55ac15a5273fad5af987717d4`.
The observations SHA-256 is
`ddec8b14f42aec0aee301f7992ef6ebe2ee8bc6f394efea680800ceca19096a2`.
This is attributed external acceptance, distinct from the earlier local Docker
measurement and from qualification of the unreleased typed-relation contract.

The application replaced five individual source reads with one existing batch
request, retaining its feed and checkpoint checks. Six sequential pairs used a
preselected randomized order, one reused real 1536-dimensional vector, identical
scope and unchanged complete context. Each query opened a fresh HTTP client and
reused its connection within the query. Concurrency was one, with no retries or
503 responses. Timing surrounded the real context-retrieval function with a
monotonic clock; it included network and consumer work but excluded endpoint
queueing/authentication, embedding generation and inference.

| Observation | Individual reads | Batch revalidation |
| --- | ---: | ---: |
| HTTP requests per context | 12 | 8 |
| Median retrieval time | 3481.605 ms | 2812.995 ms |
| Observed range | 3432.08–3570.35 ms | 2706.53–3001.40 ms |
| Measured contexts | 6 | 6 |

Recalculation from the published samples gives a 19.2% reduction in the median.
All twelve responses preserved source IDs, exact revisions, text, scores and the
context stamp. This does not establish p95, server throughput, token savings or
a reduction in an agent's full task time. Network/engine spans were not separated,
and reuse of a warm connection across queries was not compared. Six checkpoint
and feed calls plus a search remain alongside the batch; instrument those stages
before changing the consistency guarantees.

Focused 0.12.0 acceptance also covered three owned synthetic records forming a
two-edge dependency chain. Graph and batch reads agreed on transitive source
invalidation; depth-zero display preserved eligibility checks; node cuts
distinguished unexamined roots from unavailable roots. Eighteen fixture responses
and eight initial real-memory responses were validated against the served
OpenAPI. The team removed its three fixtures and reported 107 preexisting memories
unchanged, with seven fixture changes reconciled to a stable remote/SQL checkpoint.
It created no credentials and generated no new embeddings or model calls.

The team reported 402 passing integration tests, including 77 focused tests.
Production key revocation and eventless expiry were not repeated against live
credentials; those cases remain client-regression evidence. Administrative UI
acceptance is separate and remains pending. QilbeeDB context was disabled again
after the installation check while the remaining application migration and
billing work continued. This report establishes a focused API integration and
an observed latency change, not a completed memory migration or reasoning gain.
