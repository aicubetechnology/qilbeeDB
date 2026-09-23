# Share observations through an application

Use scoped memory when several authorized people or agents need to exchange
observations through your application. QilbeeDB persists the records and evaluates
their current availability. Your application decides who may participate, publish
or act on the information.

For the managed platform, use your assigned API base URL and company credentials.
For a self-hosted installation, use the URL of your running platform server and
credentials provisioned by its administrator. The requests below use paths relative
to that base URL; they do not require a model provider or a local agent runtime.

## Choose the sharing boundary

A shared scope has a project, an application-assigned agent identifier, optional
mission identifier and `visibility: shared`. The company comes from the credential.
The agent identifier may represent an authorized shared workspace. It does not
make the database the owner of that workspace's membership or business decisions.

```json
{
  "project_id": "research",
  "agent_id": "shared-observations",
  "mission_id": null,
  "visibility": "shared"
}
```

Two credential subjects with the same authorized shared scope can read the same
records. Unlike private memory, the shared namespace does not include the subject.
A grant permits access to the whole corresponding namespace. A null mission is an
exact missionless scope, not access to every mission.

Keep service credentials in your backend. Authorize the requesting participant
before querying memory and before returning its content. A `session_id` stored in
metadata, a record tag or knowledge of a record UUID is **not an access control**.
If several sessions share a namespace, your application must retain and validate
the session-to-record association. Do not expose that namespace's credential to a
participant who is only entitled to one session.

## Publish once and preserve the receipt

Use `memory_write` with a matching grant. Persist an admission record in your
application before dispatching the command. It should bind the message, permitted
author, scope, request body and stable idempotency key. Replace the illustrative
event timestamp below with the observed event time before saving that admission.

`POST /api/v1/memory/commands`:

```json
{
  "contract_version": 1,
  "idempotency_key": "session-42-observation-1",
  "scope": {
    "project_id": "research",
    "agent_id": "shared-observations",
    "mission_id": null,
    "visibility": "shared"
  },
  "operation": {
    "type": "create",
    "record": {
      "episode_type": "Observation",
      "event_time_millis": 1700000000000,
      "valid_until_millis": null,
      "content": {
        "primary": "The requested document was available when checked.",
        "data": {"evidence_ref": "document-check-42"}
      },
      "tags": ["session-42"],
      "metadata": {"session_id": "session-42", "application_message_id": "message-1"}
    }
  }
}
```

Save the returned receipt and record UUID with the admission. Persistence does
not verify the observation or promote it into an approved procedure. The record
attributes the write to the authenticated credential subject; any participant or
work identifier in metadata is an application assertion, not an independently
authenticated identity supplied by the database.

For an ephemeral observation, set `valid_until_millis` to your chosen absolute
expiry **before** saving and sending the command. For example, a one-hour session
policy may calculate its deadline from the admission time. Do not recalculate the
expiry or event timestamp when retrying. A null expiry means no configured expiry,
not guaranteed availability despite later deletion, rejection or source changes.

## Read current state before reuse

Retain the record UUIDs belonging to the authorized session. Use `memory_read`
and `POST /api/v1/memory/records/batch` with that exact scope and a `record_ids`
array containing 1–100 distinct UUIDs. The request has `contract_version: 1`.
See the complete [batch request and response](memory-batch-read.md).

For every response, verify the scope, entry count, order, IDs and current revisions.
An unavailable record has `record: null`, including after deletion, rejection or
expiry. Drop unavailable content and invalidate a cached context whose required
revision changed. Do not replace a failed current read with a cached payload and
present it as current knowledge.

A batch observes one database snapshot and evaluation time. It is not a lease
that keeps the record valid throughout a later task. Separate batches are separate
observations. The batch endpoint reads explicit IDs; it does not discover records
or filter arbitrary metadata. Exact tag filtering can help discovery but cannot
replace participant authorization and session association checks.

Use the [change feed](verified-memory-changes.md) to invalidate caches and preserve
its own cursor/checkpoint contract. Your application's presentation sequence is a
different concept. **Expiration can occur without a feed event**: reaching a feed
watermark never replaces checking current eligibility before reuse.

## Recover uncertain outcomes

| Observation | Application action |
| --- | --- |
| Command reply is lost or a connection fails | Keep the admission unresolved and retry the exact scope, subject, idempotency key and body. Do not create a new message identity. |
| Retry returns an earlier receipt | Reconcile the original commit, then read current state. A historical receipt does not imply the record is still available. |
| HTTP 401 or 403 | Stop using that authority; resolve authentication or grants before retrying. Do not fall back to a broader credential. |
| HTTP 409 | Reconcile the existing command or current revision. Do not blindly overwrite or change the key to bypass the conflict. |
| Current batch read fails | Mark the context unvalidated and preserve confirmed consumer progress. Retry according to your application's bounded recovery policy. |
| Current record is null or has a different revision | Invalidate the retained content or reconstruct the context from current eligible records. |

Idempotency is bound to the authenticated subject and namespace, not globally to
an application message ID. Credential rotation within the same subject preserves
that identity; switching subjects or scopes does not. See the full
[versioned memory contract](versioned-memory.md).

There is no transaction spanning your admission store and QilbeeDB. Your outbox
may retain the envelope, receipt and record mapping for recovery, but must not
become an alternative authority that resurrects unavailable knowledge. Keep network
calls outside application locks where a delayed response would block unrelated work.

## Correct or withdraw an observation

Update or delete through the [revision-checked command API](versioned-memory.md).
Use the [review API](memory-review.md) to record an authorized decision without
silently rewriting its provenance. Review requires `memory_review`; read/write
capabilities alone do not grant it. An unreviewed observation must not be displayed
as human-verified merely because a service successfully stored it.

Validate your integration with authorized and denied participants, lost replies,
reconnects, changed revisions, rejection, deletion and real expiry. These checks
establish integration behavior, not autonomous reasoning or improved agent ability.
