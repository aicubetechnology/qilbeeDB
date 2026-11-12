# Native episode admission and lifecycle

This guide is for applications embedding the Rust memory library. Atomic bounded
admission is an unreleased source improvement; it is not a managed-platform quota
or a new HTTP capability. Platform consumers use the [durable memory API](../api/versioned-memory.md)
and its credential authorization contract.

## Choose an application capacity

Configure an agent and an explicit maximum number of valid episodes before
creating its native `PersistentAgentMemory` manager:

```rust
let config = MemoryConfig::new("agent-id").max_episodes(1_000);
```

`store_episode` accepts episodes belonging to that configured, nonempty agent.
A zero capacity rejects writes. An update to an already valid episode does not
evict another episode. Invalidated episodes remain available through raw storage
inspection and do not occupy a valid slot; ordinary manager reads return no
invalidated episode and do not count an access.

The application chooses capacity and retention goals. Relevance is a policy
signal, not proof of truth or task utility. Native agent IDs are storage scopes;
they do not replace authenticated tenant or resource authorization.

## Atomic bounded writes

`MemoryStorage::store_episode_with_capacity(agent_id, &episode, max_episodes)`
checks ownership, the UUID's existing owner and the current valid-record count
under the backend mutation lock. If a new valid record fills an available slot,
no record is evicted. At capacity, storage chooses the current lowest relevance
score; ties select the oldest event time, then ascending UUID. The incoming
record and its index, any timestamp move, and any victim record/index deletion
commit in one write batch. A rejected scoped write or failed batch cannot leave
an eviction committed by itself.

An already valid record can be updated without consuming another slot, including
when the application's newly selected limit is below the existing count. A new
valid record is rejected while that count is already above the selected limit.
The application must reconcile its policy explicitly; lowering a limit does not
silently authorize bulk deletion. Reactivating an invalidated record consumes a
valid slot. Storing an invalidated record does not evict a valid one.

This operation serializes concurrent managers using the same backend. It checks
the limit supplied by each call; applications using different limits do not gain
a shared, persisted policy or a global quota. Direct low-level `store_episode`
and `update_episode` remain unbounded whole-record replacement operations. Use
the manager or bounded operation consistently when relying on capacity admission.
Stale whole-record updates can still overwrite newer fields: this operation is
not compare-and-swap or revision conflict detection.

Capacity admission scans the agent's current records while holding the mutation
lock. Large native stores can delay other mutations; no latency or scale target
is established by this correctness guarantee. Applications should qualify their
actual record sizes and workload before choosing a large capacity.

## Errors, compatibility and recovery

Ownership conflicts, zero capacity, malformed ranking scores, inconsistent
record/index identities and backend failures return errors. The RocksDB backend
and built-in in-memory backend implement bounded admission. The blocking HTTP
storage adapter forwards the native operation internally. Custom backends must
implement an equivalent atomic operation; the default returns an unsupported
error instead of performing separate eviction and storage calls. This preserves
Rust trait implementation compatibility while requiring explicit backend support
for manager writes.

New admission uses the existing episode format. WAL and synchronous-write
configuration determine durability; the operation does not enable them
implicitly. Retain a consistent backup before changing application policy.
Acknowledged synchronous-write recovery after killing a process does not prove
behavior under every host power-loss or disk failure. A pass involving multiple
calls is not one transaction, and a configured capacity does not protect records
from an explicit authorized deletion.
