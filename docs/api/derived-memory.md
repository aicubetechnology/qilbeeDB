# Derived memories and source revisions

Available in **0.8.0**. Use a derived memory for a summary, extracted claim or
agent-produced conclusion that should remain usable only while its recorded
sources remain current. QilbeeDB stores externally produced content; it does not
invoke a model or infer that the sources logically support the conclusion.

## Create a derived record

Call `POST /api/v1/memory/commands` with both `memory_read` and `memory_write` for
the exact scope. Use the existing command envelope with `operation.type: derive`:

```json
{
  "contract_version": 1,
  "idempotency_key": "derive-run-42",
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "operation": {
    "type": "derive",
    "record": {
      "episode_type": "Observation",
      "event_time_millis": 1700000000000,
      "content": {"primary": "The observed failure was caused by an expired credential."},
      "tags": ["derived"]
    },
    "derivation": {
      "sources": [{"record_id": "00000000-0000-4000-8000-000000000042", "revision": 2}],
      "method": "incident-summary",
      "method_revision": "prompt-v3",
      "evidence_ref": "trace://incident/42"
    }
  }
}
```

Source IDs refer exclusively to the authenticated namespace: tenant, project,
mission, agent and visibility, plus the subject for private memory. A source ID
cannot grant access to another namespace. The caller cannot supply a source
namespace, tenant or author.

`derivation` requires 1–16 unique source IDs with positive revisions. `method`
and `method_revision` are nonblank identities of at most 256 UTF-8 bytes each;
`evidence_ref` is a nonblank reference of at most 2048 bytes. Control characters
are rejected. Record the actual model/prompt/tool configuration in the referenced
evidence when needed for reproduction. These identifiers are declarations by the
writer; the database does not fetch or verify external evidence.

All sources must exist, match their exact revision, be unexpired and have no
rejected review. Sources may themselves be derived. Each root is limited to eight
dependency edges along any path and 64 distinct source IDs across its transitive
graph. Shared ancestors count once; each node retains the 16-direct-source limit.
Cycles and graphs exceeding these bounds are rejected. Source validation and creation are serialized
with memory mutations. Record, index, idempotency receipt and `derived` change
entry commit atomically with synchronous WAL durability.

The ordinary command receipt returns `action: derived`, a new `record_id` and
`revision: 1`. Reading the record includes its immutable `derivation` metadata and
authenticated content author. An identical retry returns the original receipt,
including after a source changes. Refetch current state to establish present
eligibility; a retry receipt is not a current-availability assertion.

## Correction and invalidation

A derived record remains eligible only while every recorded source and its transitive ancestors have the same
recorded revision and remain eligible. Source updates, deletions, expiry and review
revisions invalidate the dependent record before direct reads, text/filter
queries, new embedding attachment, lexical statistics, vector candidates and
hybrid combination. This check uses the same request-local storage snapshot as
the retrieval data. Requests already in progress may finish on an earlier coherent
snapshot; later requests observe committed changes.

Derived content cannot be replaced with an ordinary `update` command: it returns
409. Create a new derivation from the corrected source revisions instead. A
derived record may be deleted or reviewed. Approval of the derived record does
not override invalid source references; source approval itself advances the source
revision and therefore does not silently revive old conclusions. Every embedded
derived record still needs an external vector bound to its own current revision.

Ordinary create/update records remain independent. The database cannot infer
hidden dependencies in arbitrary text or prevent a writer from copying content
without declaring sources. Applications must select `derive` whenever they need
this provenance contract.

## Dependency work and bounded retrieval

Query, lexical, semantic and hybrid pages include `dependency_work` with
`records_examined` and `bytes_examined`. These count additional source lookups and
raw source-record bytes read for validation. Missing sources count as one lookup
and zero bytes. Integrity-index bytes and transport/allocator overhead are not
included. This work is separate from candidate scan counters and lexical/hybrid
`scan_bytes_limit`; do not interpret either counter as total process memory.

Each request caches compact source metadata within its own authorized namespace.
Shared dependencies are read once per request. There is no cross-request or
cross-scope eligibility cache. Work is limited to 4096 distinct dependency lookups
and 16 MiB of source-record bytes per request. Exceeding either limit returns 400
without a partial result; reduce the candidate `scan_limit` and use the documented
candidate cursor if appropriate. A bounded page remains a ranking over its
examined candidates, not a global top-k guarantee. Oversized individual sources
may require a smaller source representation rather than a smaller candidate page.

## Events, history and limitations

Source mutations produce their own [change-feed](memory-changes.md) events.
Serving-time invalidation does not rewrite descendants or emit a separate event
for each affected derived record. A cache must track reverse dependencies or
invalidate the scope when its dependency set is unknown. Clock-driven expiry
produces no event: revalidate before use and respect source validity.

Source references identify exact revisions but this API does not retain historical
payloads of ordinary memory updates. Preserve required evidence outside this
mutable record API or in immutable evidence artifacts. A source revision is not a
permanent content-download promise. Metadata does not certify a conclusion's
truth, relevance or downstream benefit.

Absent sources return 404; changed or ineligible source revisions return 409.
Invalid source lists or graphs exceeding depth/node bounds return 400. Authentication, grants,
integrity errors, retry rules and the 64 KiB request body limit follow the
[durable memory API](versioned-memory.md).

## Explain a dependency failure

A reviewer can call `POST /api/v1/memory/eligibility` with `memory_review` and the
exact scope. This uses the same authorization as metadata-only review state;
ordinary read or write authority alone is insufficient.

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "record_id": "00000000-0000-4000-8000-000000000042"
}
```

The response is `{contract_version, scope, eligibility}`:

| Field | Meaning |
| --- | --- |
| `record_id`, `revision` | Current root identity, including unavailable roots |
| `eligible` | All serving prerequisites were established in this request |
| `evaluated_at_millis` | Server time used for validity checks |
| `first_failure` | Null when eligible; otherwise the first failed root or dependency check |
| `dependencies_checked` | Distinct source IDs visited for this root, at most 64 |
| `max_depth_examined` | Maximum dependency path depth encountered or inferred from a previously checked shared subtree |
| `all_dependencies_checked` | True only when the complete bounded graph was validated |
| `dependency_work` | Actual additional reads and raw record bytes, using the same work limits as retrieval |

`first_failure` contains `record_id`, nullable `expected_revision`, nullable
`actual_revision` and `reason`. Reasons include `deleted`, `expired`, `rejected`,
`source_missing`, `source_revision_changed`, `dependency_cycle`, `depth_limit`
and `node_limit`. `eligible` is the successful internal reason and never a
failure. A reference revision is checked before that source's current disposition:
if a source was rejected at a newer revision, an older reference reports
`source_revision_changed`. Null `actual_revision` means it was not established;
it does not mean revision zero.

The diagnostic stops at the first failed check in the stored source-list traversal
order. It is not a complete inventory of every issue. A bound failure means the
service cannot establish eligibility within its admitted graph contract. It does
not assert that an unvisited source is factually wrong. Depth may exceed eight in
the diagnostic that reports a bound failure; creation rejects that graph.

No content, vector, method identity, evidence reference or reviewer identity is
copied into this response. A missing root returns 404. Existing deleted, expired,
rejected or source-invalid roots return 200 with `eligible: false`. The result is
an observation of one snapshot and one time, not a durable authorization token or
a guarantee of eligibility on a later request. Restore or regenerate a conclusion
only after checking its current sources and creating a new derivation.

Traversal caches compact source metadata and validated subtree heights. This
avoids repeatedly expanding shared subgraphs while still enforcing the longest
path bound when the same subtree appears at different depths. All normal serving
paths use this transitive check, including BM25 corpus construction. There is no
asynchronous descendant rewrite or eventual-consistency window introduced by a
background propagation worker.

## Traverse source ancestry

The 0.12.0 [memory evidence graph API](memory-evidence-graph.md) reads current
`derived_from` ancestry from explicit roots in one authorized snapshot. It
returns eligible records and exact revision-bound edges, deduplicates shared
sources, and reports traversal coverage. Display limits never skip the complete
eligibility checks required for a returned record.
