# Durable HTTP memory

The HTTP episode routes now use the existing RocksDB memory backend through
`PersistentAgentMemory`. One shared store lives at `<data-directory>/agent-memory`;
agent IDs are database key components, never filesystem paths. Blocking storage
calls run through Tokio's blocking pool. The default HTTP path does not evict
records through an implicit business quota; retention is separate policy work.

WAL and synchronous writes are enabled for HTTP episode mutations. Startup
returns an error if the memory store cannot open; it does not fall back to RAM.
`http_server::create_router` therefore now returns `qilbee_core::Result<Router>`.
Applications embedding the router must handle that error before serving traffic.

## Existing endpoint behavior

- `POST /memory/{agent}/episodes` persists the episode. The body `agentId` must
  match the path. Context, structured `content.data`, supported property metadata
  and a valid `eventTime` are retained. An out-of-range timestamp is rejected.
- `GET /memory/{agent}/episodes/{id}` parses a UUID and uses the durable ID index.
  It can retrieve records older than the most recent 100 episodes. Invalid UUIDs
  return 400; absent or invalidated records return 404.
- Recent reads, existing text queries, statistics and maintenance access the
  same persistent backend after reopening. Empty namespaces return empty lists
  or zero statistics without requiring a process-local registry.
- `DELETE /memory/{agent}` removes that agent's stored episodes durably.

This is a persistence foundation for the [P0 contract](../research/delivery-acceptance.md),
not completion of that contract. The legacy POST still assigns a new episode ID
per call: callers must not treat retries as idempotent yet. Versioned conditional
updates, stable receipts, authenticated tenant/sharing scopes and procedural
HTTP operations remain subsequent features. Existing data held only in an old
process's RAM cannot be recovered after that process exits.

The existing legacy semantic endpoints still require a separate contract fix;
their synthetic rank scores do not establish semantic similarity. Production
credential/bootstrap and resource authorization gaps also remain. Do not infer
production readiness from the persistence change.

## Validation and fault model

Five HTTP reproductions failed before implementation. Seven tests now cover
reopen, old-ID lookup, malformed IDs, durable clear, structured content and
metadata, mismatched agent input, storage-open failure and abrupt restart.

The abrupt-restart test starts a child process serving real HTTP on an ephemeral
loopback port, acknowledges 20 writes, kills the process without a graceful
shutdown, restarts it, and checks every ID and structured value plus the total
count. The subprocess fixture is ignored in ordinary test enumeration and is
invoked explicitly by its parent test. All children and data are temporary.

This tests process termination on the local filesystem. It does not establish
hardware power-loss, filesystem corruption, disk exhaustion or backup guarantees.

```sh
cargo test --workspace --all-targets --locked
```

At this feature snapshot: **328 tests passed**, with one intentionally ignored
subprocess fixture. No paid model calls, external agent benchmarks or QMN runtime
services are required.
