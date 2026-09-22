# Rebuild the native memory index safely

`PersistentAgentMemory::rebuild_vector_index` rebuilds the Rust library's
in-process semantic index from the valid episodes returned by its storage
backend. It returns the number of episodes indexed, or an error. This native
library operation is separate from platform HTTP retrieval and does not alter
HTTP ranking or authorization contracts.

## Publication and failure

Rebuilding prepares a separate index. The published index remains available
while episodes are loaded and the configured native embedding adapter prepares
vectors. Every preparation error is propagated. Only after every loaded episode
has been indexed does one write-lock operation replace the live index.

If an embedding request fails, insertion fails, or the rebuild future is dropped
before publication, the old index remains intact. A failed rebuild no longer
returns a successful partial count. A successful rebuild over no valid episodes
publishes an empty index and returns zero.

The operation does not roll back embedding-provider requests or charges already
incurred. Cancelling the local future does not prove cancellation at an external
provider. Configure timeouts and account for incomplete external outcomes in the
application. No background rebuild worker is started by this method.

## Concurrent access

Clones of the same memory manager share an asynchronous index-mutation gate.
`index_episode`, `unindex_episode` and rebuild preparation acquire that gate.
Consequently, an insert or removal queued behind a rebuild runs after the
replacement; an older prepared index cannot subsequently overwrite that
completed mutation. A second rebuild waits before loading its source episodes.
Cancellation releases the gate.

Queries continue using the previously published index during preparation. The
synchronous index read lock is released before fetching episodes from storage.
The final index swap briefly requires an exclusive lock. This is not a latency
service-level guarantee: CPU, storage, embedding latency and corpus size still
matter. A slow provider blocks queued index mutations until the operation ends
or is cancelled, while readers can continue using the prior index.

## Source consistency limits

The gate coordinates index operations on one manager and its clones. It is not
a transaction over the storage backend. `storage()` exposes a backend that
other writers may mutate independently, and ordinary episode writes do not
participate in this index gate. Independently constructed managers do not share
it either.

The returned count describes the valid source episodes loaded for that rebuild,
not a snapshot guaranteed current at publication. An episode changed after
source loading may therefore require another index update or rebuild. Current
storage validity is checked when resolving search results, but this feature
does not bind a vector to a source revision or prove that an old vector matches
new content. Do not use successful reconstruction as evidence of that stronger
consistency guarantee.

Applications that require a stable source for reconstruction must coordinate
all their writers for the operation. Preserve source data and investigate errors
before retrying; this operation does not repair or delete durable episodes. The
staged index also requires additional memory alongside the previous index and
the loaded episode list. Test capacity with representative workloads.

## Validation scope

Controlled tests cover provider failure, cancellation after preparation starts,
invalid dimensions after an earlier candidate was successfully indexed, reader
availability, clone insertion/removal ordering, competing rebuilds and empty
publication. The failure tests were reproduced against the previous
implementation before correction. The insertion-failure case compares the
published index bytes before and after the failed attempt.

These tests use local controlled embedding adapters, not paid inference or
production data. They establish the tested publication and ordering behavior;
they do not measure retrieval quality, agent improvement, storage-wide snapshot
consistency or distributed index recovery.
