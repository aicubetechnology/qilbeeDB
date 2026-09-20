# Current retrieval candidates

Lexical, semantic and hybrid search use the `current_records_v1` candidate
selection plan in QilbeeDB 0.10.0. This plan prevents deleted memories from
consuming the candidate budget and selects exact tag and episode-type partitions
before reading memory content or vectors. Tenant, project, mission, agent and
private subject authorization still determine the namespace before candidate
selection. Embeddings remain external.

## What changes for a client

Continue to use the existing endpoints and server-owned ranking versions.
`cosine_exact_v1` retains exact float64 cosine accumulation over float32 vectors;
BM25 and both weighted RRF profiles retain their formulas and tie breaking.
This change does not introduce approximate neighbors or a new relevance claim.

The page includes these fields in all three search modes:

| Field | Meaning |
| --- | --- |
| `candidate_selection_version` | `current_records_v1`; record this with server and ranking versions in comparisons |
| `scanned_records` | Current candidates admitted from the authorized tag/type partition |
| `candidate_index_bytes` | Logical candidate key/value bytes admitted, separate from source bytes |
| `scanned_bytes` | Serialized current records and selected-space bindings admitted; now also available for semantic search |
| `next_after` | Exclusive UUID of the last examined current candidate if more candidates remain |
| `exhaustive` | True only when a cursorless request covers the complete current-candidate partition |

`scan_limit` limits `scanned_records`, including semantic candidates that have no
binding in the requested model space. `scanned_embeddings` counts only bindings
actually decoded for eligible current candidates. A stale binding can be decoded
but cannot contribute a score. Missing bindings never acquire an inferred model
identity. Hybrid `embedded_records` and `embedding_coverage` retain their meaning.

Page fields are additive; clients with closed response schemas must use the
published 0.10.0 OpenAPI. Scores retain their previous units. When comparing
versions, keep the candidate selection version in the report: the examined
population and work counters differ from the legacy all-record/all-binding scan.

## Maintenance and upgrades

Creation, update, tag/type changes, deletion and review maintain the candidate
projection in the same synchronous WAL batch as the canonical memory, integrity
index, receipt and change event. A deleted or directly rejected record has no
candidate entry. Historical receipts, tombstones and vectors remain durable;
search does not enumerate them as candidates. Updating a memory invalidates the
old revision's vector for scoring until a new binding is attached.

The server checks a per-namespace journal fingerprint at startup. An absent or
stale fingerprint triggers reconstruction from canonical records before serving
requests, including after the journal-aware 0.9.0 version wrote to the database. Downgrading
to a binary that changes canonical memories without updating the journal is not
a supported write path. Legacy namespaces without a journal are rebuilt at every
open. Reconstruction
uses batches of at most 256 source records, holds no corpus-sized collection and
publishes its completion marker last. An interrupted reconstruction restarts
safely. Errors abort startup; an incomplete projection is never advertised ready.
Unchanged namespaces are skipped by seeking over their key ranges.

Allow time and disk headroom for this migration. A namespace that needs rebuilding
requires a scan of its canonical records, including tombstones. The 256-record
batch limit bounds individual writes; it does not bound the total startup time.
The projection adds storage and write work proportional to the number of tags.
It is a candidate filter, not a token inverted index or an ANN structure.

## Coverage and operational limits

Expiry and transitive source eligibility are evaluated in the request snapshot.
A record that expires after indexing, or becomes ineligible because a source
changes, can still consume the candidate budget. This release does not implement
background expiry reclamation or recursive removal from this projection. Such
records contribute neither hits nor BM25 statistics. Check `exhaustive` and
`dependency_work`; do not assume that live-record count alone guarantees complete
coverage under every policy and history.

The byte counters are logical serialized work. They exclude integrity-index
reads, RocksDB amplification, allocations and one lookahead entry; dependency
reads are reported separately. They are not network traffic, RAM or physical disk
I/O. Lexical/hybrid byte limits still apply, and semantic work remains bounded by
candidate count and embedding dimensions. Record latency, CPU and memory under
the actual workload before increasing production limits.

A cursor traverses current candidates, not ranked hits or a retained snapshot.
Each continuation takes a new snapshot. An unchanged corpus advances without
duplicate UUIDs, but concurrent changes can alter subsequent pages. Continued
pages remain non-exhaustive. **Do not combine BM25 or hybrid page scores into a
global ranking.** Prefer a complete filtered corpus within the agreed budgets.

Use `scripts/benchmark_retrieval_history.py` only against a dedicated disposable
scope. Its synthetic 1,536-dimensional vectors measure coverage and read work;
they do not measure relevance or agent-task improvement.
