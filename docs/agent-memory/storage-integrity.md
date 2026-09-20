# Durable episode integrity

This feature applies to `RocksDbMemoryStorage`, used by
`PersistentAgentMemory`. The existing HTTP memory routes use `AgentMemory`
and remain volatile; this change does not make those routes persistent.

## Guarantees

- An episode's embedded agent ID must match its storage scope. IDs cannot be
  reassigned to another agent. Wrong-agent reads return no result; wrong-agent
  deletion returns `false` without touching the owner’s index.
- Moving an episode's event time atomically deletes its old row and updates
  its row and UUID index in a single RocksDB write batch.
- A per-storage mutex serializes index read/modify/write operations. Cloned
  `Arc` handles share this lock; RocksDB excludes a second independent opener.
  This does not provide optimistic conflict detection for application-level
  read/modify/write sequences.
- All mutation paths honor WAL and synchronous-write settings. With
  `sync_writes=true` and `enable_wal=true`, acknowledged writes request WAL
  synchronization. Disabling WAL reduces durability. Explicit flush covers
  all memory column families.
- Index parsing checks lengths before accessing bytes and returns a corruption
  error on malformed records.

## Record format and compatibility

New episode values have the six-byte `QMEP\0\x01` header followed by a bincode
tuple `(Episode, Option<String>)`. The episode's structured `data` field is
temporarily empty inside the binary component; the second component contains
its JSON representation. This avoids bincode's lack of `deserialize_any`
support, required by `serde_json::Value`, without changing episode keys or
binary property values.

Unversioned bincode episodes without structured data remain readable and are
upgraded on subsequent writes. Unknown envelope versions fail explicitly.
Legacy records already written with structured JSON could not be decoded by
the original implementation and are not repaired by this feature. Downgrading
to an older binary after writing new records is unsupported; back up before
upgrading. No existing database is rewritten automatically.

## Validation

Eight regression tests in `storage.rs` cover cross-agent delete protection,
owner mismatch and UUID reassignment, timestamp moves, structured JSON across
reopen, malformed indexes, legacy reads/updates, concurrent moves, invalid
scopes and unknown versions. Five original reproductions failed before the
fix. Validation commands:

```sh
cargo test --workspace --all-targets --locked
rustfmt --edition 2024 --check crates/qilbee-memory/src/storage.rs
```

Reopen tests exercise normal process shutdown and recovery, not power-loss or
filesystem fault injection. The existing HTTP authorization, graph transaction
atomicity and application-level access-counter races require separate work.
