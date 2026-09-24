# Offline store verification

`qilbeedb verify-store` inspects a stopped data directory and reports whether
its stores are complete, readable by this binary and internally consistent. Use
it after an offline backup, before adopting a restored or migrated copy, and
when qualifying a release against a copy of production data. It applies to
self-hosted installations and to operators of a managed platform who hold a
copy of the data directory; platform users without file access do not need it.

## What it checks

The command opens the three stores of a platform data directory read-only:

| Store | Directory | Checks |
| --- | --- | --- |
| Graph and identity | `<data-directory>` | Exact column families, physical inventory |
| Agent memory | `<data-directory>/agent-memory` | Exact column families, physical inventory |
| Procedural learning | `<data-directory>/procedural-learning` | Schema marker, authoritative inventory, knowledge index consistency |

For every column family it reports the number of records and a SHA-256 digest
over the record bytes. The digest is deterministic for the same records
regardless of write order, compaction or file layout, so two reports compare a
source with its copy. Learning-store entries that the server derives on start
(the active knowledge index and its generation marker) are counted separately
and excluded from the digest; that digest therefore stays equal across restarts.

The knowledge index check re-derives every locator and active ranking entry
from the authoritative procedure, receipt and combined-origin ledgers with the
same audit the server runs on start. Each implied entry must exist with
identical bytes, every persisted entry must belong to the published generation,
and the counts must match. Receipt and origin digests, key identities, origin
linkage and bound procedures without receipts are all rejected.

## Run it

Stop the server first and confirm its exit. Then run the same binary version
that wrote the data, for example inside the container image with the volume
mounted:

```sh
docker run --rm --network none \
  --mount type=volume,source=<data-volume>,target=/data \
  <image> verify-store /data
```

For a native installation:

```sh
qilbeedb verify-store /srv/qilbeedb/data
```

Success prints one JSON document and exits with status 0:

```json
{
  "contract_version": 1,
  "status": "verified",
  "verifier_version": "0.14.0",
  "data_directory": "/data",
  "stores": {
    "graph": {"path": "/data", "families": [{"family": "nodes", "records": 12, "sha256": "…", "derived_entries": 0}]},
    "agent_memory": {"path": "/data/agent-memory", "families": ["…"]},
    "procedural_learning": {
      "path": "/data/procedural-learning",
      "families": [{"family": "default", "records": 41, "sha256": "…", "derived_entries": 9}],
      "knowledge_index": {"generation": "…", "knowledge_receipts": 4, "combined_origins": 1, "active_entries": 3, "derived_entries": 9}
    }
  }
}
```

Any failure prints a message on standard error, prints no report and exits
with status 1. The command never writes to a store, never repairs an index and
never creates a missing directory.

## Refusals and their meaning

| Message | Meaning and next step |
| --- | --- |
| `open by process <pid>` | A server or another tool holds the store. Stop it; do not verify a live store. |
| `column families … expects …` | The directory was written by a different version or is not this kind of store. Use the matching binary. |
| `Knowledge index generation is missing` | The learning store was never closed by a binary with this index. Start and stop the matching server once, then verify. |
| `Knowledge index entry is missing` / `differs` / `ledgers imply` | The derived index does not match the ledgers. Keep the copy for inspection; a writable start would rebuild the index, but the underlying cause must be understood first. |
| `digest mismatch`, `key mismatch`, `origin` errors | Authoritative records were altered or partially copied. Do not adopt the copy. |
| `Corruption` mentioning a log or SST file | RocksDB found damaged bytes. A copy with a damaged write-ahead log is rejected outright rather than silently reported as a shorter store. After a power loss the tail of the log may be torn: keep the original, start the matching server once on a copy so it recovers, then verify that copy. |

The lock probe detects stores held by other processes only. A store opened for
writing inside the same process is the caller's error and is not detected.

## Compare a source with a copy

Run the command on the stopped source and on the stopped copy, then compare
`records` and `sha256` per family. Graph and agent-memory digests include the
projections that a writable start rebuilds deterministically, so they match
between faithful copies of the same version; after upgrading the binary those
projections may legitimately change, while the learning store's authoritative
digest must not. Keep both reports with the backup manifest.

## Limits

The command validates structure and integrity, not business meaning: it does
not prove that retained memories are the ones an application expects, that a
backup is recent enough, or that retrieval quality is unchanged. Agent-memory
journal chains, checkpoints and consolidation ledgers are inventoried, not
re-derived. Cloud snapshot consistency, freeze handling and access control for
the copy remain operator procedures described in
[backup and recovery](backup.md).
