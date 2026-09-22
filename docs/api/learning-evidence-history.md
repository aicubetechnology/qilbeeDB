# Inspect learning evidence and historical outcomes

**Availability: 0.14.0.** Check the running server’s `/openapi.json` before
integrating these endpoints; older installations require an upgrade.

Start with a procedure, strategy, experience or development request discovered
in the [company learning catalog](company-learning-catalog.md). Its recorded
history explains the inputs, authors and outcomes retained by the database.
An administrator can discover and select that evidence without knowing case or
event IDs in advance. The original writer need not remain active.

## Choose the evidence that answers your question

| Selected resource | Evidence kind | What the record establishes |
| --- | --- | --- |
| Procedure or strategy | `evaluation_submission` | The submitted measurements, authenticated evaluator identity and admission decision, including rejected, incomplete, cancelled, pending and unknown-consumption outcomes. |
| Procedure or strategy | `paired_evaluation` | A retained paired-measurement receipt and the procedure state immediately after that comparison. Includes direct trusted-library evaluations without inventing an authenticated credential author. |
| Experience | `experience_event` | An immutable observation, its original actor, evidence digest, reported consumption and resulting historical attempt revision. |
| Development request | `tool_development_event` | A worker report or cancellation request, its actor and resulting historical development revision. A cancellation request is distinct from worker-confirmed cancellation. |

Accepted admission means the submission entered the configured evaluation
contract. It does not mean that the procedure was promoted, that measurements
were independently reproduced, or that a task succeeded. A paired receipt can
also appear inside its accepted submission; these two views are not independent
trials. Do not add their row counts together as an evaluation sample size.

Unknown consumption stays null. Keep reported consumption separate from retained
lower bounds. A historical success is not proof that a procedure is currently
active or that an external executor is safe. Re-read the parent resource for its
current state and use its lifecycle contract for reuse decisions.

## Inspect history in the administration console

In the corresponding unreleased console candidate, open **Experiences**,
**Learning & policies**, or **Learned tools**, choose a resource type, and select
an entry by its displayed title. Choose **View evidence history**. The selected
resource's project, agent and private owner remain visible above the history.
The history uses the available reading width and one page scrollbar.

For procedures and strategies, choose **Evaluation submissions** or **Paired
comparisons**. Search by title, status or identifier, or browse the entries.
Open an entry to inspect its original decision, actor when recorded, measurements
and historical state. Consumption without a known value is shown as **Unknown**.
Use **Original receipt and technical identity** to export the exact contract.

**Back to evidence list** returns focus to the selected entry. **Back to current
resource** returns to its observed details; use **Refresh details** for a new
observation before deciding whether to reuse it. Escape returns one level.
Searches and retries preserve a predictable keyboard focus. A failed inspection
clears the receipt until a confirmed retry; a failed continuation preserves the
previous entries and continuation. Loss of administration access clears the
protected catalog and details.

The console stops at 500 loaded evidence entries. Narrow the search or start a
fresh traversal to examine a different subset. The visible count is the number
loaded, not a claim of total history size. Inspect **Evidence coverage and
ordering** for the last page's bounds and ordering semantics.

## Authorization

Both endpoints require a currently valid company credential or login session
with `credential_admin`. The server derives the company from that credential.
The request cannot choose another company or supply a physical namespace. Pass
the complete parent `resource` from the catalog, including project, agent,
mission, visibility and private subject. A selection or cursor never grants
access. Equal IDs in another company or private subject remain separate.

These are administrative reads. They do not issue delegated credentials,
acknowledge events, advance checkpoints, register agents, execute tools, generate
embeddings or change a learning decision. Unsupported combinations of parent and
evidence kind return `400`. Missing parents or selected receipts return `404`.

## Discover evidence for the selected resource

Send `POST /api/v1/company/learning/evidence/query`:

```json
{
  "contract_version": 1,
  "query": {
    "resource": {
      "kind": "procedure",
      "id": "recovery-procedure",
      "scope": {
        "project_id": "research",
        "agent_id": "analyst",
        "mission_id": null,
        "visibility": "private"
      },
      "private_subject_id": "researcher"
    },
    "kind": "evaluation_submission",
    "limit": 25,
    "max_scanned_records": 100
  }
}
```

The page contains entries with an exact `evidence` selection, a short title,
status, optional historical revision and server recording time. Titles normalize
whitespace and retain at most 160 Unicode characters from submitted detail or
evidence references; development events use the recorded reason. Titles are
labels, not unique identities or a replacement for the original receipt.

Optional `query.text` performs a case-insensitive substring match over the
displayed title, evidence ID and status. It does not search full payloads or
perform semantic retrieval. Omit it or use null to browse all evidence of the
selected kind.

## Inspect the exact receipt

Select an entry and send its complete `evidence` object to
`POST /api/v1/company/learning/evidence/read`:

```json
{
  "contract_version": 1,
  "evidence": {
    "resource": {
      "kind": "procedure",
      "id": "recovery-procedure",
      "scope": {
        "project_id": "research",
        "agent_id": "analyst",
        "mission_id": null,
        "visibility": "private"
      },
      "private_subject_id": "researcher"
    },
    "kind": "evaluation_submission",
    "id": "case-42"
  }
}
```

The response echoes `company_id` and `evidence`, and returns `details.kind` and
its typed `details.record`. Verify the response selection before displaying it.
The record preserves original measurements, references, actors and historical
states; it does not inherit the parent's later approval or execution status.
References are not fetched or verified against external providers by this API.

## Continue without losing progress

Pass `page.next_cursor` unchanged as `query.cursor`, with the same complete
resource, evidence kind and text filter. Changing page or scan limits is allowed;
changing the selected parent or filter requires a fresh traversal. The cursor
binds the authenticated company and those selections.

Entries follow storage-key order, which is neither chronological nor alphabetical
display order. Use each recording time and revision to interpret its history.
Pages serialize with learning writes but do not share a snapshot or revision
fence. Inserts behind a cursor require restarting discovery. A cursor is not a
journal checkpoint or a restore-detection witness. Use the verified memory feed
and checkpoint contracts for incremental context consumers.

An empty filtered page with a continuation is incomplete discovery. It does not
prove that there is no evidence. On a failed next page, preserve the last confirmed
entries and exact continuation. Retry that continuation; do not replace it with
an unconfirmed response. Clear the rendered details after an inspection failure
or lost authorization, and ignore late replies from an earlier selection.

## Coverage, work limits and integrity

The default result limit is 25, with a maximum of 50. The default scan budget is
100 primary records, with a maximum of 1,000. A 4 MiB primary key/value budget
can stop a page earlier. A single primary record above that ceiling fails the
request. Requests share retrieval admission and briefly serialize with learning
writes.

| Page field | Meaning |
| --- | --- |
| `stop_reason` | `exhausted`, `entry_limit`, `scan_limit` or `byte_limit` |
| `next_cursor` | Continuation when a limit stopped this page; null at the end of the observed prefix |
| `scanned_records` | Primary evidence records examined, including text-filtered entries |
| `scanned_record_bytes` | Bytes in those primary keys and values |
| `observed_at_millis` | Server observation time for the page |

Counters exclude dependency verification, parent inspection, RocksDB internal
work and lookahead. They are not total CPU, memory, response-size or latency
measurements. Integrity checks use the selected parent's immutable bindings and
kind-specific readers. Admission inspection checks the original actor and
submission against its contract and paired receipt; development events check
command, actor, report, artifact and recorded outcome consistency. Encountered
inconsistency fails the whole request instead of returning a partial successful
list. These checks do not establish an externally authenticated archive of all
past transitions or validate unvisited records.

## Errors and recovery

| Status | Action |
| --- | --- |
| `400` | Correct fields, bounds, unsupported evidence kind or cursor binding. Restart after changing selections. |
| `401` / `403` | Clear protected data and recover currently authorized company access. |
| `404` | Refresh the parent catalog or evidence page; the selected record is unavailable. |
| `413` | Reduce the request; these endpoints use the 65,536-byte request limit. |
| `500` | Treat the operation as failed integrity/storage inspection. Do not present previous details as current evidence. |
| `503 retrieval_busy` | Retry with bounded backoff and the same confirmed continuation. |

Authentication and company-administration authorization precede admission to
retrieval work. All responses use `Cache-Control: no-store`. Qualification must
cover discovery, selection, typed detail, partial pages, errors, authorization
loss, keyboard navigation and mobile recovery before a console is published.
Qualification evidence is recorded separately from release availability. This
guide does not claim a measured improvement in agent reasoning or formal
accessibility conformance.
