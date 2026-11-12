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
| Graph and identity | `<data-directory>` | Exact column families, physical inventory, property-index reconstruction |
| Agent memory | `<data-directory>/agent-memory` | Exact column families, authoritative inventory, native episode identity/index checks, journal chains, candidate, chronological and relation projection checks |
| Procedural learning | `<data-directory>/procedural-learning` | Schema marker, authoritative inventory, knowledge index consistency |

For every column family it reports the number of records and a SHA-256 digest
over the record bytes. The digest is deterministic for the same records
regardless of write order, compaction or file layout, so two reports compare a
source with its copy. Independently verified derived entries are counted as
`derived_entries` and excluded from the digest: in the learning store the
active knowledge index and its generation marker; in the agent-memory store the
candidate and chronological projection rows and tips. Workspace membership and
relation metadata remain included in the digest. Relation metadata keeps exact
byte comparison even when its current projections pass the independent audit.

The graph property-index check reconstructs the expected index from stored nodes
using a verification-specific implementation of the canonical fingerprint. It
checks node identities, the supported index format, expected keys and values,
and unexpected entries. Floating-point bit patterns and nested property values
are preserved in the comparison. Two copies containing the same invalid index
must fail verification, even when their raw inventories match. The report adds
`stores.graph.property_index` with `nodes` and `index_entries` counts.

This check does not rebuild an index or exempt graph records from byte comparison.
The audit refuses values over 16 MiB, more than 1 GiB of examined key/value
bytes, more than two million nodes or expected entries, or more than 256 MiB
of retained expected keys. Serialized property nesting is limited to 64 levels
and collection items to one million per decoded record. These are verifier work
limits, not database storage quotas; exceeding one produces no success report.
RocksDB materializes each returned value before its size is checked, so these
limits are not a hard ceiling on process memory.

It verifies the current format; it does not authorize a migration from an older
index format. Keep the original data and qualify any format migration separately.

**Native episode identity/index verification is available in development source.**
Older verifier binaries inventoried these native rows without checking their
relationship. Select a qualified release that includes this audit before relying
on it to accept a native-memory backup.

The native episode check requires every retained episode's owner, UUID and event
time to match its complete primary key. Its UUID lookup must point to exactly
that location, and every lookup must resolve to a retained episode. Missing,
orphaned, duplicate-location, malformed or misdirected entries fail, including
invalidated episodes that remain retained. The audit reads supported raw legacy,
QMEP-v1 and QMEP-v2 records through the native decoder without upgrading them.
Platform records sharing these column families keep their separate checks.

The existing report fields are unchanged; `projections` counts platform
projections, not native episodes. Native primary and lookup rows remain included
in the authoritative family digests. The command never repairs a lookup or
deletes an unindexed record. Preserve a rejected copy and diagnose its origin;
matching inventories alone cannot establish that two copies are consistent.

The projection check re-derives the candidate and chronological projections
of every namespace from the canonical records with the same key and value
builders the server uses. Each implied row must exist with identical bytes,
each namespace's projection tips must equal the digest of its journal state,
and the persisted row counts must equal the implied counts, so an extra or
stale row fails, including rows in namespaces with no canonical records.

**Relation projection auditing is available in development source.** Older
verifier binaries counted relation heads without checking their contents. Use a
qualified release containing this audit before relying on it to validate a
typed-memory graph projection.

Every current memory revision must have outgoing and incoming completeness
heads, and every current memory namespace must have a relation projection tip
matching its journal fingerprint. Current relation records are checked against
their integrity records and latest history/receipt, and each indexed relation
must have both expected adjacency rows with identical integrity bytes. Retired
or rejected relations must have neither row. Expiry and endpoint staleness do
not remove a retained relation's physical index obligation: eligibility remains
a separate read-time decision.

The audit recomputes each head's count and digest from its physical adjacency
rows. Global counts reject extra or orphaned integrity, adjacency and tip rows,
including rows outside namespaces reached from current records. Complete head
keys, schema, namespace, endpoint identity, positive revision and direction are
validated. Head revisions cannot exceed the corresponding retained memory's
current revision. Current and relation-endpoint heads are required; valid empty
historical heads may survive updates or disappear during a startup rebuild.
This lifecycle does not require a head for every unreferenced historical revision.

`relation_head_entries` now counts heads that passed these checks. Report fields,
byte inventories and source/copy comparison rules are unchanged. The audit never
rebuilds a projection or deletes an extra row. Keep a rejected copy unchanged for
diagnosis before considering a separately qualified recovery operation.

**Complete physical journal coverage is available in development source.**
Older binaries checked the recorded tip and anchored suffix more narrowly;
use the qualified release's guide when selecting a verifier.

The journal check visits every namespace that owns a journal state, a change
record or an anchor. It requires canonical keys and exactly the physical rows
implied by each recorded range: legacy events from sequence one through the tip,
and verified anchors strictly after the baseline through the tip. Missing rows,
extra rows before or beyond those ranges, malformed keys and orphaned rows fail.
A gap does not hide a later extra event or anchor.

Every legacy event is checked against its journal identity, sequence and existing
event digest, including events before a verified baseline or in a legacy-only
journal. The verified state and anchor chain retain their baseline, tip, event
digest and previous-link checks. Validating legacy events does not retroactively
turn that prefix into a cryptographically linked history; the verified baseline
remains an upgrade boundary. These are internal consistency checks, not an
external authenticity or source-truth attestation.

The report fields remain unchanged: namespaces, legacy journals, verified
journals and `links_checked`. The last field counts verified anchor links, not
legacy events or physical-key checks. Verification never deletes extra rows or
repairs a copy; preserve a rejected copy for investigation.

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
| `Property index is inconsistent or exceeds offline verification limits` | The graph index format, node identity, reconstructed keys/values, or verification budget did not pass. Keep the stopped data for diagnosis; the verifier does not repair or silently truncate. |
| `Native episode or UUID index is inconsistent` | A native record cannot be decoded, its owner/UUID/event time disagrees with its primary key, or the bidirectional UUID lookup audit failed. Keep the stopped copy and original bytes for diagnosis; do not delete records to make verification pass. |
| `Knowledge index generation is missing` | The learning store was never closed by a binary with this index. Start and stop the matching server once, then verify. |
| `Unsupported or inconsistent memory record, index or receipt` | A memory journal, record, index or relation projection failed integrity verification. Keep the copy for inspection; do not adopt it. |
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

The native episode audit streams native primary and lookup rows with point
lookups, retaining one decoded episode at a time. Its work grows with stored
rows and payload sizes. RocksDB and the decoder materialize stored values, so
this is not a hard process-memory or duration bound. It checks readable identity
and lookup consistency, not every application-level field or an external
cryptographic attestation of a native payload. A validly encoded alteration that
preserves identity may need comparison with a trusted retained source to detect.

Journal processing grows with the actual stored journal rows and bytes. The
verifier retains coverage counters and identifiers per namespace, then checks
physical cardinality before walking a recorded sequence range. A large recorded
tip cannot by itself cause a walk beyond the observed physical row count.
Retained metadata grows with distinct namespaces, and RocksDB materializes each
returned row. This is not a hard process-memory or duration ceiling; plan an
offline window appropriate to the stored history. Online feed page limits and
wire contracts are unchanged.

The relation audit scans stored records, current relations, heads and adjacency
rows, with point reads for required identities and integrity data. It retains
one head aggregate at a time and can read a referenced record more than once.
RocksDB and decoding materialize values; there is no hard process-memory or
duration ceiling. A forged count or revision does not cause a walk over an
unstored numeric range. All historical relation revisions and command receipts
are still inventoried; only the latest history/receipt needed by each current
relation is independently checked by this projection audit. Internal consistency
does not establish that an assertion or its cited evidence is externally true.

The command validates structure and integrity, not business meaning: it does
not prove that retained memories are the ones an application expects, that a
backup is recent enough, or that retrieval quality is unchanged. Agent-memory
journal chains are re-walked and candidate and chronological projections are
re-derived, and current relation projections are audited; checkpoints, consolidation ledgers and vector
indexes are inventoried, not re-derived. Cloud snapshot consistency, freeze handling and access control for
the copy remain operator procedures described in
[backup and recovery](backup.md).
