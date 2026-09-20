# Agent-scoped episode lifecycle

The synchronous `AgentMemory` and asynchronous `PersistentAgentMemory` managers
now enforce the same ordinary-access rules:

- A stored episode must belong to the configured, nonempty agent ID. This is
  checked before capacity checks or eviction, including with a custom backend.
- A zero `max_episodes` rejects writes rather than accepting an unbounded first
  episode.
- Updating an existing valid episode at capacity does not evict another record.
- Capacity concerns valid episodes; invalidated records in the in-memory map do
  not consume a live slot.
- `get_episode` returns no result for an invalidated episode and does not
  increment its access counter. Raw storage access remains available for
  administrative inspection; temporal/history APIs require separate design.

Seven regression tests exercise both managers. Ownership rejection, invalidated
reads and update-at-capacity behavior failed before the fix. The API changes
are intentionally stricter: clients that previously read invalidated episodes
through ordinary access must use explicit administrative storage access.

These checks are not user authentication or tenant authorization. The
persistent manager still performs quota checks, eviction and storage through
separate backend calls; concurrent managers or backend failures can break a
global capacity invariant. Transactional capacity enforcement, atomic access
counters and durable index maintenance remain separate work.
