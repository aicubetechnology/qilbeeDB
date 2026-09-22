# Property index equality and upgrades

Status: unreleased 0.14.0 candidate. This is a Rust graph storage contract, not
an additional HTTP search mode. It does not change embedding, cosine or hybrid
ranking semantics.

## Finding equal properties

A property index narrows candidates; the stored property is still compared with
the requested value before a node is returned. Equal maps must find the same node
regardless of map insertion or iteration order. Signed floating-point zero also
compares equal: `-0.0` and `0.0` share an index fingerprint, including inside
arrays, maps and spatial coordinates.

The format `canonical-fnv1a64-v1` uses tagged values, fixed-endian numeric fields,
length-delimited strings and collections, recursively sorted map keys, and
normalized signed zero. Arrays retain their order. The FNV-1a 64-bit fingerprint
is deterministic candidate selection, not a cryptographic digest, a uniqueness
constraint or proof of equality. Collisions require checking stored values.
NaN retains Rust's existing non-equality semantics; this feature does not define
new application-level numeric equivalence.

## Opening an existing installation

Before exposing a storage handle, an installation without the index-format marker
reconstructs the entire property index from authoritative node records. This also
removes obsolete property keys left by earlier map updates or deletions. Entity
records, graph identities, relationships, memory records and audit history are
not rewritten. Reconstruction does not make retired graphs accessible.

The database clears the old property index, then writes reconstructed keys in
batches flushed when their encoded size reaches 1 MiB. One individual entry may
exceed that threshold. Memory includes the current node, its properties and the
RocksDB caches; this is not a total process-memory limit. Each migration batch uses
a synced WAL even if ordinary runtime writes have weaker settings. The completion
marker is committed with the final batch.

An interruption before completion leaves no marker. The next opening clears the
partial index and restarts reconstruction. It does not serve partially indexed
queries. A malformed node key, mismatched node identity, unreadable node or storage
write error fails opening; retain the error and repair from verified source data
or backup. Do not delete source records merely to bypass an upgrade failure.
An unknown format marker also fails opening without replacing the index.

## Operational procedure

1. Stop writers and create a verified, application-consistent backup of the whole
   database. Keep the previous binary and backup together.
2. Rehearse the upgrade on a disposable copy with the exact target image. Measure
   startup time, disk growth, CPU and memory. Reconstruction scans all stored
   nodes, including retained graph data; large installations need an appropriate
   maintenance window and free disk space for WAL and compaction.
3. Start the upgraded database. Wait for successful readiness before connecting
   clients. Repeatedly killing a slow but progressing migration restarts its work.
4. Verify exact property queries, map and signed-zero queries, update/delete
   behavior, scoped memory access and unrelated graph/memory state.
5. Resume writers only after those checks. Preserve the backup and migration
   evidence according to the operator's retention policy.

**Do not run an older binary against the upgraded directory.** Older versions do
not understand the new fingerprint and may silently miss results or introduce
mixed index keys. Rollback means restoring the complete pre-upgrade backup with
its matching binary; writes accepted after that backup require explicit recovery
and reconciliation. Deleting the format marker is not a supported downgrade.

## Validation scope

Regressions reproduce missing equal maps and signed-zero values on the prior
implementation. Tests cover recursive values, coordinate zeros, legacy and
partial keys, stale-key removal, update/delete, reopening, unknown format and
corrupt source refusal. A child process is killed after a synced partial
reconstruction; reopening rebuilds all 300 fixture nodes and the next reopening
retains them. This verifies process interruption, not physical power loss,
corrupted disks, exhaustive interruption points or production sizing.
