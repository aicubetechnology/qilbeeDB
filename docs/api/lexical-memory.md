# Lexical memory search

Use `POST /api/v1/memory/search/lexical` to retrieve durable memories by BM25.
This mode needs text and an authorized scope, with no embedding model or vector.
It is useful for identifiers, error codes and exact words in memory content.
For combined lexical and semantic evidence, use [hybrid retrieval](hybrid-memory.md).

## Send a lexical query

```json
{
  "contract_version": 1,
  "mode": "lexical",
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "query": {
    "text": "ZX17 retry failure",
    "limit": 10,
    "scan_limit": 10000,
    "scan_bytes_limit": 8388608,
    "after": null,
    "episode_type": null,
    "tag": null
  }
}
```

Send a platform bearer credential with `memory_read` and an exact grant for the
scope. The response contains `contract_version: 1`, `scope`, `mode: "lexical"`,
`ranking_version: "bm25_v1"` and `page`. Existing durable memories are searchable
without an index migration or an embedding attachment.

## Read results and coverage

Each `page.hits` entry contains its current `record` and positive raw BM25 `score`.
Scores order results; they are not probabilities. Equal scores use ascending
record UUID. `matched_records` counts matches before result truncation,
`corpus_records` counts visible records passing filters, and `scanned_records`
includes deleted, expired and filtered source rows examined. `scanned_bytes`
counts serialized source records admitted to the scan.

`exhaustive` is true only when a cursorless request scans the whole authorized
scope. `next_after` is the last scanned source UUID if another row remains.
This cursor traverses source rows, not ranked results, and each continuation
uses a new snapshot. Continued pages always report `exhaustive: false`.

BM25 statistics are calculated from the filtered scanned corpus. **Do not merge
scores from separate pages into a global ranking.** Narrow the scope or increase
the scan budget, within implementation limits, when a complete ranking is needed.
The exclusive cursor prevents duplicate source UUIDs across forward pages, but
concurrent writes can change subsequent pages. This is a full scan, not an
inverted index or a promise of sublinear query time.

## Scoring and validation

`bm25_v1` splits primary, secondary and context text on non-alphanumeric Unicode
characters, lowercases terms and deduplicates query terms. It uses no stemming,
stopword removal or canonical Unicode normalization. It does not search metadata.
Document length includes all tokens in those fields. The fixed formula uses
`k1 = 1.2`, `b = 0.75` and positive inverse document frequency:

```text
idf(term) = ln(1 + (N - df(term) + 0.5) / (df(term) + 0.5))
score = sum(idf(term) * tf(term) * 2.2 /
            (tf(term) + 1.2 * (0.25 + 0.75 * length / average_length)))
```

`N` and `df` use only visible records passing the exact scope and query filters.
Data in another tenant, project, mission, agent or private subject cannot affect
scores. All reads use one RocksDB snapshot and visibility timestamp. There is no
cross-scope search cache. Deleted and expired records contribute neither hits nor
statistics. Source corruption fails the request instead of dropping a candidate.

| Field | Accepted values |
| --- | --- |
| `text` | 1–4096 UTF-8 bytes with 1–64 distinct lowercase alphanumeric terms |
| `limit` | 1–100 results |
| `scan_limit` | 1–10000 source records, default 10000 |
| `scan_bytes_limit` | 1–268435456 bytes, subject to the operator ceiling (default 67108864); request default 8388608 |
| `after` | Optional exclusive source UUID cursor |
| `episode_type`, `tag` | Optional exact filters, applied before statistics |

The byte budget excludes keys, integrity indexes, allocator overhead and one
lookahead row. It is not a process memory limit. A budget that cannot fit its
first source row returns 400 rather than a non-advancing cursor. These ceilings
bound this implementation; they are not configurable enterprise quotas.

Malformed requests, punctuation-only text, mode mismatch, invalid limits and
unknown fields return 400. Missing, revoked or expired credentials return 401;
missing capability or scope returns 403; oversized JSON bodies return 413;
encountered storage inconsistency returns 500. Use the shared
[error envelope](platform-http.md). JSON requests have a 65536-byte body limit.

## Measure retrieval time

The response includes `timing.retrieval_micros`, the server wall time spent in
the retrieval method. It excludes authentication, blocking-pool queueing, JSON
serialization, transport and external embedding generation. Measure the client
round trip separately. See the [reproducible evaluation workflow](../research/retrieval-evaluation.md)
for frozen corpora, graded relevance, category regressions and timing limits.

## Execution capacity

The ranking catalog exposes the current server dimension, scan-byte and concurrent
retrieval limits. These are operator settings, separate from the immutable ranking
profile and from tenant authorization. A byte budget above the configured ceiling
returns 400 (`retrieval_scan_limit`); exhausted retrieval slots return 503
(`retrieval_busy`). Use bounded backoff and inspect coverage on each successful
response. See [configure retrieval capacity](../operations/retrieval-capacity.md).

## Source-dependent eligibility

[Derived memories](derived-memory.md) validate their declared sources in the
request snapshot before candidate eligibility and corpus statistics. Pages report
additional source reads in `dependency_work`; these are separate from candidate
scan budgets. Exceeding the documented dependency limits fails the request.
