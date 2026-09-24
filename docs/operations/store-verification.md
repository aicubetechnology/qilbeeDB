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
| Agent memory | `<data-directory>/agent-memory` | Exact column families, authoritative inventory, journal chain verification, projection re-derivation |
| Procedural learning | `<data-directory>/procedural-learning` | Schema marker, authoritative inventory, knowledge index consistency |

For every column family it reports the number of records and a SHA-256 digest
over the record bytes. The digest is deterministic for the same records
regardless of write order, compaction or file layout, so two reports compare a
source with its copy. Independently verified derived entries are counted as
`derived_entries` and excluded from the digest: in the learning store the
active knowledge index and its generation marker; in the agent-memory store the
candidate and chronological projection rows and tips. Workspace membership and
relation heads remain included in the digest because this command does not
independently re-derive their contents.

The projection check re-derives the candidate and chronological projections
of every namespace from the canonical records with the same key and value
builders the server uses. Each implied row must exist with identical bytes,
each namespace's projection tips must equal the digest of its journal state,
and the persisted row counts must equal the implied counts, so an extra or
stale row fails, including rows in namespaces with no canonical records.
Relation heads are counted, not re-derived.

The journal check visits every namespace that owns a journal state, a change
record or an anchor. It validates the legacy change journal (last change digest
and no dangling successor), the verified journal state (baseline digest, tip
anchor and no dangling successor) and walks the anchor chain from baseline to
tip with the same digest and link checks that the verified change feed applies.
The report counts namespaces, legacy journals, verified journals and links
checked. Orphaned anchors or changes without their state record fail.

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
    "agent_memory": {
      "path": "/data/agent-memory",
      "families": ["…"],
      "journals": {"namespaces": 3, "legacy_journals": 3, "verified_journals": 3, "links_checked": 128},
      "projections": {"namespaces": 3, "candidate_entries": 256, "chronological_scope_entries": 128, "chronological_company_entries": 128, "relation_head_entries": 40}
    },
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
| `differs from source … in:` | The copy and the source are not the same data; the message lists each store and family. Do not adopt the copy as equivalent. |
| `Writer exclusion` | The command could not retain every store lock, the helper failed, or a lock file changed. Keep all source and candidate stores stopped, inspect the named path when provided, and retry only after resolving the cause. |
| `column families … expects …` | The directory was written by a different version or is not this kind of store. Use the matching binary. |
| `Knowledge index generation is missing` | The learning store was never closed by a binary with this index. Start and stop the matching server once, then verify. |
| `Unsupported or inconsistent memory record, index or receipt` | A memory journal chain, anchor or change record does not verify. Keep the copy for inspection; do not adopt it. |
| `Knowledge index entry is missing` / `differs` / `ledgers imply` | The derived index does not match the ledgers. Keep the copy for inspection; a writable start would rebuild the index, but the underlying cause must be understood first. |
| `digest mismatch`, `key mismatch`, `origin` errors | Authoritative records were altered or partially copied. Do not adopt the copy. |
| `Corruption` mentioning a log or SST file | RocksDB found damaged bytes. A copy with a damaged write-ahead log is rejected outright rather than silently reported as a shorter store. After a power loss the tail of the log may be torn: keep the original, start the matching server once on a copy so it recovers, then verify that copy. |

Before reading, the command acquires the existing RocksDB locks for all three
stores, or all six stores when comparing a source and a copy. It retains them
until verification and comparison finish, then confirms their release before
printing a success report. New cooperating writers are excluded throughout
that interval. Partial acquisition or loss of the lock-holder process fails
without a success report.

Run on a local POSIX filesystem with working record locks and permissions to
open the existing `LOCK` files for reading and writing. The command does not
create missing lock files or modify database records. Do not replace directories,
rename lock files, or bypass filesystem locking while verification runs.
Directories sharing the same lock inode are rejected.

## Compare a source with a copy

Pass the stopped source with `--source` to verify both directories in one run
and require that they hold the same data:

```sh
qilbeedb verify-store /restore/data --source /srv/qilbeedb/data
```

Both directories are verified exactly as in the single-directory form. The
command then requires equal record counts and digests for every column family
of the three stores, equal memory journal counts and equal knowledge index
counts; knowledge index generations may differ because each writable start
republishes one. Success prints a report with `status` `verified_equal`, both
full reports and a `comparison` summary, and exits 0. Any difference exits 1
with a message naming each differing store and family, for example
`agent_memory/memory_agent_meta, agent_memory/journals`, and prints no report.
The two paths must be distinct and must not contain each other.

Only independently verified derived projections are excluded from digests.
Other projections remain byte-compared; differences after an upgrade require
review rather than an assumption of equivalence. Graph digests include the
property index that a writable
start maintains, so a named `graph/` difference after an upgrade needs review.
Keep the comparison report with the backup manifest.

## Limits

The command validates structure and integrity, not business meaning: it does
not prove that retained memories are the ones an application expects, that a
backup is recent enough, or that retrieval quality is unchanged. Agent-memory
journal chains are re-walked and candidate and chronological projections are
re-derived; checkpoints, consolidation ledgers, relation heads and vector
indexes are inventoried, not re-derived. Cloud snapshot consistency, freeze handling and access control for
the copy remain operator procedures described in
[backup and recovery](backup.md).
