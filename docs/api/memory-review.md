# Review and correct memories

Available in **0.8.0**. Review decisions let an authorized application approve,
reject or mark a memory unreviewed while preserving who decided, the evidence
reference and the exact resulting revision. Decisions and their journal entries
are durable. Check `/health` before using these routes.

## Grant review authority

All three review routes require `memory_review` and an exact resource grant.
`memory_write`, `memory_read` and tenant administration alone do not grant review.
Tenant, reviewer subject and credential are derived from authentication. Private
memories remain limited to the credential's own subject, including for reviewers.
A human review application and an automated evaluator can use the same contract;
the database does not infer that an authenticated subject is a human.

## Submit a revision-checked decision

Call `POST /api/v1/memory/reviews` with a bearer credential:

```json
{
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "command": {
    "contract_version": 1,
    "idempotency_key": "review-case-42",
    "record_id": "00000000-0000-4000-8000-000000000042",
    "expected_revision": 1,
    "disposition": "rejected",
    "evidence_ref": "case://quality/42"
  }
}
```

`disposition` is `approved`, `rejected` or `unreviewed`. The nonblank
`evidence_ref` is a caller-supplied reference of at most 2048 UTF-8 bytes, without
control characters. It must not contain secrets. QilbeeDB stores this reference;
it does not fetch the resource or verify the truth of the decision.

A successful response contains `contract_version: 1` and `receipt` with:

| Field | Meaning |
| --- | --- |
| `idempotency_key` | The request identity, scoped to namespace and authenticated subject |
| `record_id`, `revision` | The memory and its new current revision |
| `review.disposition` | The recorded decision |
| `review.evidence_ref` | The evidence reference supplied in the command |
| `review.author` | Authenticated reviewer credential and subject |
| `review.reviewed_at_millis` | Server commit time in Unix milliseconds |
| `receipt_digest` | Namespace-bound integrity digest, not a signature or proof of truth |

The receipt also carries `contract_version: 1`. Review increments the revision
without changing the content or its original `author`. The record's
`modified_at_millis` and optional `review` metadata reflect the decision. Record,
integrity index, historical receipt, retry receipt and `reviewed` change event
commit in one synchronous WAL batch. Competing decisions for the same expected
revision have only one winner.

Retry the identical command and idempotency key after a lost response. It returns
the original receipt even after later changes, deletion or credential rotation
within the same subject. Reusing the key for another command returns 409. A retry
receipt describes the original commit; it is not a claim that this is still the
current decision. Authentication and current grants are checked on every retry.

## Read current state and historical decisions

Ordinary reads return 404 for rejected memories. A reviewer can obtain
metadata-only state with `POST /api/v1/memory/reviews/state`:

```json
{
  "contract_version": 1,
  "scope": {"project_id": "project", "mission_id": null, "agent_id": "agent", "visibility": "shared"},
  "record_id": "00000000-0000-4000-8000-000000000042"
}
```

The response is `{contract_version, scope, state}`. `state` contains `record_id`,
`revision`, nullable `review`, `deleted` and `expired`. Use this revision for a
new decision. It exposes no memory content. Expiry is evaluated against the
request's server time; a deleted record has no payload from which to derive an
expiry and reports `expired: false` alongside `deleted: true`.

Call `POST /api/v1/memory/reviews/read` with the same fields and `revision` set to
the resulting revision of a past decision. It returns the original receipt. A
revision produced by an ordinary create, update or delete has no review receipt
and returns 404. The [change feed](memory-changes.md) identifies decision revisions
through `reviewed` entries; there is no unbounded history-list operation.

## Serving, correction and embeddings

A rejected revision is excluded before lexical corpus statistics, vector
candidate eligibility and hybrid combination. The same gate applies to direct
reads, text/filter queries and new embedding attachment. Requests already using
a database snapshot may finish with the coherent pre-decision state; subsequent
requests observe the committed decision. Clients must invalidate or revalidate
cached results when processing its change event.

Every decision advances the revision, including `unreviewed` and repeated new
commands recording the same disposition. Previous embeddings therefore become
stale. After approval or clearing a review, attach an externally generated vector
to the new revision before expecting semantic coverage. The database never assigns
old vectors to a new revision automatically. Lexical retrieval can resume without
a new embedding.

A content update through the ordinary memory command API clears the current
review: approval and rejection concern a specific revision, not all future
content under the same ID. An authorized writer can publish corrected content as
a new unreviewed revision; unreviewed records remain eligible. If an application
requires approval before every publication, it must enforce that workflow and
restrict writer credentials. This contract does not provide an approval-required
serving policy. Historical decisions remain readable after correction or deletion.

Approval does not extend expiry, restore deleted content, change access, certify
facts or qualify a learned procedure. Review of a deleted record returns 404;
review of an existing expired record records the decision but leaves it expired.

## Errors and limits

Unknown fields, invalid decisions, nonpositive revisions and malformed evidence
return 400. Invalid/revoked credentials return 401; missing capability or grant
returns 403. The state/read endpoints return 404 with `review_not_found` when
record state or the requested immutable review revision is absent in the authorized
scope. An existing unreviewed record has a readable state but no historical review
receipt. Stale expected
revisions and conflicting idempotency keys return 409. Detected integrity failure
returns 500 without a partial result. Requests use the standard 64 KiB body limit
and responses use `Cache-Control: no-store`.

Stored memories from older releases have no review metadata and remain unreviewed.
Review history is retained without automatic pruning in this release. Decisions
are assertions by authorized reviewers; independent evaluations are still needed
to measure the effect on answer quality and agent outcomes.

Use the metadata-only [eligibility diagnostic](derived-memory.md#explain-a-dependency-failure)
to identify the first transitive source that prevents serving a derived memory.
This diagnostic also requires `memory_review`.
