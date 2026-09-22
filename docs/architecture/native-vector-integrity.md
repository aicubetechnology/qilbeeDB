# Native vector index integrity

The Rust `qilbee_memory::HnswIndex` is an approximate in-process vector index.
This guide describes its numeric input, replacement and snapshot contracts. It
is separate from the platform HTTP cosine and hybrid retrieval paths: this
change does not alter HTTP scores, ranking versions, tenant authorization or
embedding identity requirements.

## Validate before indexing

Use `HnswIndex::try_new(config)` to report invalid configuration immediately.
The existing `new(config)` constructor remains source-compatible; insert, search
and snapshot operations validate its configuration before using it. The native
agent's `with_semantic_search` also rejects invalid index configuration.

Vectors must be nonempty, finite and match the configured dimension. If no
dimension is configured, the first accepted insertion establishes it. Rejected
input does not establish dimensions or replace an existing vector. Cosine also
requires at least one nonzero component. Zero vectors are permitted for the
other two metrics.

The native index does not impose a 1,536-dimension ceiling. Tests cover cosine
at 3,072 and 32,768 dimensions, including the smallest positive `f32` and
`f32::MAX`. These are correctness cases, not latency or relevance benchmarks.
Memory consumption and retrieval time still depend on dimensions and corpus
size. Callers supply vectors; this index does not generate embeddings or verify
that two vectors belong to the same model space.

Configuration requires `m >= 2` with representable `2*m`, positive construction
and search candidate counts, a finite positive `ml`, a positive explicit
dimension, and `max_level < usize::BITS`. The existing level-generation policy
is unchanged by this feature.

## Distance representation

Products, sums and norms use `f64` intermediates; returned distances remain
`f32`. Smaller distances rank first.

| Metric | Returned distance | Accepted magnitude |
| --- | --- | --- |
| Cosine | `1 - cosine`, bounded to `[0, 2]` | Any finite, nonzero `f32` vector |
| Dot product | Negative dot product | Squared vector norm no greater than `f32::MAX` |
| Euclidean | Euclidean distance | Vector norm no greater than `f32::MAX / 2` |

The dot-product and Euclidean norm bounds apply to stored vectors and queries.
They conservatively ensure that distances between accepted inputs fit the
public `f32` result type. Inputs outside these bounds return an error instead
of introducing infinite distances. This tightens acceptance of extreme legacy
inputs; normalize or otherwise transform vectors consistently outside the
index, then rebuild, if the application's model permits that transformation.

A request for `k` results uses at least `max(ef_search, k)` search candidates.
It is no longer silently capped at `ef_search`. This does not guarantee `k`
results or exhaustive nearest neighbors: the corpus may be smaller or its
reachable graph may contain fewer nodes. Increasing `k` increases search work;
callers must impose appropriate resource limits.

## Replace and remove

Inserting an existing identifier replaces its vector while preserving its
assigned level and existing neighborhood before refreshing connections. It does
not create self-connections. This prevents an entry-point replacement from
first discarding the graph's outgoing links. It does not guarantee that updates
preserve measured recall across arbitrary datasets.

Removal clears every incoming reference, including links whose reverse edge
was previously pruned. Removing the last record clears the entry point and
maximum occupied level. Removal scans the graph; this is an integrity contract,
not a constant-time deletion claim. Remaining connectivity and retrieval quality
must be evaluated for the application's update/deletion workload.

## Load and recover snapshots

The serialized field layout remains unchanged. A fixed-width legacy fixture is
loaded, searched and written back byte-for-byte in compatibility tests.
Snapshots must describe an existing entry point at the highest occupied level,
consistent dimensions and node identities, valid per-node layer shapes, and
neighbors that exist at the referenced layers. Input/configuration validation
also applies to loaded vectors. Empty snapshots cannot retain a live entry point
or occupied level. Trailing bytes are rejected.

Invalid snapshots return an error; the loader does not silently discard records
or repair topology. Preserve the original bytes and rebuild a separate index
from authoritative vectors and identifiers. Validate its contents and retrieval
before replacing the old index. Previously accepted inconsistent snapshots or
out-of-range vectors may now require this recovery procedure.

Structural validation does not authenticate snapshots, prove graph connectivity,
prove exhaustive retrieval or impose an overall process memory budget. Accept
snapshot files only from trusted storage and enforce size/resource limits at the
application boundary. This in-process serializer is not itself an atomic disk
backup or crash-recovery protocol.

## Evidence and limitations

Regression tests cover invalid-input rejection, finite high-dimensional cosine,
requested counts beyond `ef_search`, entry-point replacement, broken snapshot
references and dimensions, asymmetric deletion, empty-index recovery and legacy
layout compatibility. These results demonstrate the tested integrity contracts.
They do not establish better relevance, agent reasoning, production HTTP
performance or state-of-the-art ANN performance.

The architectural reference is Malkov and Yashunin's
[HNSW paper](https://arxiv.org/abs/1603.09320). This implementation's construction,
level policy and update workload need separate comparative qualification before
making algorithm-equivalence or performance claims.
