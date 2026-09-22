# Resolve native vectors against their source

The native Rust memory adapter binds indexed vectors to the source episode used
for their preparation. On retrieval it compares that source with the episode
read from storage before returning a score. Replacing content under the same ID
therefore cannot attach an old vector's score to the replacement content.

This is an in-process library contract. It does not change platform HTTP
retrieval, authorization, embedding ownership or server ranking versions.

## Admission and source comparison

`index_episode` rejects invalid episodes and episodes owned by a different
agent. Index insertion and source publication share the index write lock.
Clones share the index and source bindings. Reconfiguring semantic search creates
a new index and a new binding collection for that manager.

The source comparison includes ID, agent, episode type, event and transaction
times, primary and secondary content, context, structured data, supplied content
embedding, metadata, consolidation state and invalidation state. Relevance and
access counters are excluded: an ordinary access does not invalidate the vector.
Map order does not affect equality. NaN-bearing values fail equality
conservatively rather than proving a matching source.

Queries capture each vector hit and its source together, release synchronous
locks, and then read authoritative episodes. Missing, invalid, foreign or changed
sources are not returned. Reindexing the current valid episode restores its
eligibility. A change after the authoritative read remains possible: this is
correspondence to the observed source, not revision CAS, immutable subsequent
state or a transaction spanning all result reads.

## Observe coverage

The existing `search_by_embedding` returns the valid result list as before.
Use `search_by_embedding_report` when inspecting coverage or evaluating retrieval.
Its `NativeSemanticSearchReport` exposes:

| Field | Meaning |
| --- | --- |
| `indexed_records` | Records in the native index when candidate selection occurred |
| `candidates_selected` | ANN hits selected before resolving their sources |
| `unbound_candidates` | Selected hits without a source binding |
| `missing_sources` | Bound hits whose source is absent from storage |
| `invalid_sources` | Sources invalidated or owned by another agent |
| `mismatched_sources` | Otherwise eligible sources changed since indexing |
| `results` | Hits with matching, currently observed eligible sources |
| `exhaustive` | Always false for this approximate retrieval path |

The selected count equals returned results plus the four discard counts. It is
not the number of graph nodes examined, the number of all relevant documents or
a recall estimate. The indexed count can include records whose sources were
subsequently removed or changed. Errors from storage are returned as errors,
not converted into a successful partial report.

Source checks happen after ANN candidate selection. Discards can reduce the
result count and can leave other valid records outside the selected set. The
method does not silently scan or over-fetch the entire corpus to fill the page.
Do not describe reduced coverage as a ranking improvement. Reindex/rebuild and
evaluate candidate budgets explicitly for your workload.

## Reconstruction and cancellation

A rebuild prepares both a candidate index and its source bindings. The final
publication holds both locks and replaces both collections without an intervening
await. Failure or cancellation before publication retains the previously
published vectors and bindings. Index mutation ordering follows the
[native reconstruction contract](native-index-rebuild.md).

Sources may change while preparation runs. A successfully published candidate
still describes the episodes it loaded. The query-time comparison rejects a
source that changed in the meantime; rebuilding does not certify a storage-wide
snapshot. A controlled regression changes a source while preparation is paused,
verifies that resolution discards it, then reindexes and verifies recovery.

## Memory cost and capacity

The initial representation retains an `Arc<Episode>` source snapshot per indexed
record. It favors exact typed comparison over a compact digest. Content and
metadata are cloned during admission; a rebuild retains its old published
sources alongside the candidate sources until publication. In-flight queries
can keep references to old snapshots until they finish. This is additional
memory beyond durable source storage and vector/graph storage.

A local controlled experiment indexed 128 records with 8-dimensional mock
vectors. It compared live requested Rust heap bytes added by a bare HNSW index
with those added by the bound native index after storage and input fixtures were
already allocated:

| Primary text bytes per record | Bare index bytes | Bound index bytes | Paired difference |
| ---: | ---: | ---: | ---: |
| 64 | 236,868 | 317,588 | 80,720 |
| 4,096 | 236,796 | 833,444 | 596,648 |
| 65,536 | 249,876 | 8,699,828 | 8,449,952 |

These are single-run diagnostic allocation observations, not RSS, peak rebuild
memory, production sizing or a confidence interval. The two indexes have
independently generated graph topology; their allocation differences are not a
perfect isolation of binding overhead. Nevertheless, the observed growth shows
why full payload retention must be budgeted. The fixture has no large metadata
or supplied embedding payloads; those can increase the cost further. Set source
and corpus limits in the application and qualify actual peak memory before
broad adoption. This change does not add an automatic process memory ceiling.

## Validation limits

Tests cover changed content/provenance under one ID, foreign-agent rejection,
access-only updates, invalidation/deletion, a partition of all selected candidate
dispositions, reconstruction-time source changes, reindex recovery and retention
of original bindings after cancellation or late candidate failure. These are
integrity tests, not measured relevance or agent-quality improvements. Native
binding state is reconstructed in memory and is not a new durable snapshot
format or a cryptographic authentication mechanism.
