# Inspect company consumer progress

Company administrators can discover retained memory and relation consumers without
obtaining a writer's API key or entering a consumer identifier. The company is
always derived from the authenticated credential. A request cannot choose another
company. These read-only operations neither acknowledge changes nor advance or
repair a checkpoint.

## Discover consumers

Send `POST /api/v1/company/memory/consumers/query` with an administrative bearer
credential and a JSON body:

```json
{
  "contract_version": 1,
  "query": {
    "kind": "memory_v2",
    "limit": 25,
    "max_scanned_records": 100
  }
}
```

Choose `memory_v2` for verified memory progress, `relations_v1` for relation
progress, or `memory_v1` for legacy memory progress. Optional `text` matches
consumer, checkpoint owner, project, agent, mission, and private owner identifiers
without case sensitivity. Feed kinds remain separate; an empty page for one kind
does not establish that the company has no consumers.

Each entry includes the complete consumer selection, stored revision, sequence,
and update timestamp. The checkpoint owner (`subject_id`) is distinct from the
private memory owner (`private_subject_id`). Two owners may retain independent
progress for the same consumer name in a shared workspace.

Discovery reads existing checkpoint primary keys and requires no registration or
backfill. A checkpoint can be discovered even when its workspace has no retained
memories. A retained checkpoint does not establish that a worker is running.
Clients that have never stored server-side progress are not inventoried here.

## Continue discovery

The response contains `page.entries`, `scanned_records`, `scanned_record_bytes`,
`observed_at_millis`, `stop_reason`, and an optional `next_cursor`. Pass the exact
cursor in `query.cursor` while preserving the company, feed kind, and text filter.

Each request allows 1–50 returned entries, 1–1,000 examined checkpoint primaries,
and at most 4 MiB of primary key/value bytes. The byte counter does not represent
all RocksDB I/O: integrity checks can read associated journal or receipt records.
The default limits are 25 entries and 100 examined primaries. Authorization bounds
the company key range before scanning, filtering, and accounting.

`entry_limit`, `scan_limit`, or `byte_limit` means discovery must continue. An empty
filtered page may still carry a continuation. `exhausted` means that this request
reached the end of its selected range. It is not a company-wide snapshot, worker
health assessment, or proof of completeness across all feed kinds.

A page is read consistently from one storage snapshot. Continuations traverse
live primary-key order and resume after the last examined key; they do not retain
a snapshot across requests. Concurrent insertions behind that position require a
fresh traversal. A discovery cursor is not a change-feed cursor or recovery witness.

## Inspect a selected consumer

Send `POST /api/v1/company/memory/consumers/read` with `contract_version: 1` and
`consumer` copied from a discovered entry. An optional `witness` has a matching
verified feed `kind` and its exact `cursor`. Legacy memory progress does not
accept a verified witness. Missing retained progress returns `404 record_not_found`.

Verified observations report current journal initialization, high watermark,
stored checkpoint, history compatibility, and pending sequence positions when
compatibility can be established. `active` describes journal initialization, not
worker liveness. Pending positions are not a count of reusable memories or external
effects. Incompatible history is visible for diagnosis; inspection does not repair
it. A supplied witness can identify a lost history branch even when sequence
numbers have caught up after restoration.

Legacy observations expose `position_in_range` and an optional
`sequence_distance_estimate`. These compare journal identity and sequence bounds;
they cannot verify a history branch. Do not label legacy progress as verified,
or treat a missing estimate as zero pending work.

Refresh before acting on an observation. Consumer progress may change immediately
after the response. These endpoints do not issue credentials, retry external
effects, revoke access, or overwrite confirmed progress.

## Use the administration console

Open **Consumers** in a company administrator session. Choose **Verified memory**,
**Graph relationships**, or **Legacy memory**, then browse the discovered rows or
search by consumer, owner, project, or agent. Each row identifies the checkpoint
owner separately from the memory's private owner. You do not need a writer's key.

Select a consumer to inspect its current saved progress. Verified diagnostics
show whether the checkpoint matches current history; legacy diagnostics explicitly
label their sequence estimate as unverified. Refresh the inspection before making
a recovery decision. The console clears old details if refresh fails and offers a
retry; loss of authorization clears the directory and inspection together.

Use **Continue discovery** when more checkpoints remain. A failed continuation
keeps already observed rows and retries the same cursor. An empty filtered page
can still require continuation. **Clear search** or another stream can reveal
other retained consumers. **Back to consumers** and Escape return to the selected
row. Technical evidence and explicitly scoped application tools are available
under expandable sections; routine browsing requires no manual identifiers.
