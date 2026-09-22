# Durable HTTP memory

This page describes the **legacy** memory routes. Since Rust version 0.2.0,
the [platform router](platform-http.md) is the default; legacy routes require
explicit configuration and retain their known authorization limitations.

The HTTP episode routes now use the existing RocksDB memory backend through
`PersistentAgentMemory`. One shared store lives at `<data-directory>/agent-memory`;
agent IDs are database key components, never filesystem paths. Blocking storage
calls run through Tokio's blocking pool. The default HTTP path does not evict
records through an implicit business quota; retention is separate policy work.

WAL and synchronous writes are enabled for HTTP episode mutations. Startup
returns an error if the memory store cannot open; it does not fall back to RAM.
`http_server::create_legacy_router` therefore returns `qilbee_core::Result<Router>`.
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

For authenticated tenant scopes, conditional updates and stable retry receipts,
use the [versioned memory API](versioned-memory.md). The legacy POST assigns a
new episode ID on each call; do not retry it as though it were idempotent. Data
held only in a terminated process's RAM cannot be recovered by enabling durable
storage afterward.

## Recovery limits

Persistence does not by itself guarantee recovery from host power loss, filesystem
corruption or disk exhaustion. Maintain backups and validate recovery with the
storage and deployment configuration you operate. See [backup and recovery](../operations/backup.md).
