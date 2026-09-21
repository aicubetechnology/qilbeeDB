# Automatic agent registration

**Availability:** 0.12.0 contract. Versions through 0.11.0 do not expose this
directory. Check `/health` and the installation's published OpenAPI before using
`GET /api/v1/agents`.

Your application assigns each agent its ID. On the first successful authorized
resource request carrying that ID, QilbeeDB durably records the association
between the authenticated company and the unchanged external ID. There is no
separate enrollment request and no QilbeeDB-generated business identifier.

Use a [company integration policy](company-integrations.md) to allow new IDs
without adding an exact grant for each agent. Existing exact-grant credentials
also participate when their requested scope is authorized. Registration does not
grant access: company, capability, project, agent, mission, visibility, and private
subject checks still run on every resource request.

## Make the first request

Issue an integration credential with `memory_write` and a policy permitting your
project, agent, mission, and visibility. Then send the usual memory command:

```json
{
  "contract_version": 1,
  "idempotency_key": "consumer-run-928-first-observation",
  "scope": {
    "project_id": "support",
    "agent_id": "consumer-agent-928",
    "mission_id": null,
    "visibility": "private"
  },
  "operation": {
    "type": "create",
    "record": {
      "episode_type": "Observation",
      "event_time_millis": 1700000000000,
      "content": {"primary": "The agent observed a recoverable tool failure."},
      "tags": ["tool-failure"],
      "metadata": {"source": "consumer-run-928"}
    }
  }
}
```

Send this body to `POST /api/v1/memory/commands` with the integration bearer
credential. HTTP 200 retains the existing memory receipt contract. The new memory,
its receipt, retrieval metadata, change event, and first agent association commit
in the same synchronous WAL-backed memory batch. Validation or batch-preparation failure creates neither
the memory nor its agent association. A commit error or lost response requires
idempotent reconciliation, as described below.

The company is derived from authentication. `agent_id` is an opaque,
case-sensitive identifier of 1–256 UTF-8 bytes, with no control characters and at
least one non-whitespace character. QilbeeDB does not trim, normalize, or replace
it. Your application must use the same ID for the same company agent across
projects. An identical string in another company identifies a different agent.

An authorized resource read can also register an agent, including an empty
successful memory query. Authentication failure, scope denial, malformed input,
missing requested resources, and other failed operations do not register an
agent merely because an ID appeared in the request. Health checks, login,
credential inspection, and administrative directory reads do not enroll agents.

Private memory remains bound to the authenticating credential's subject. The
agent ID is not a private owner selector, and first registration does not transfer
ownership. The registered subject records who made the first successful observed
request; it is not an exclusive right to use the ID. Other credentials in the
same company may use that ID when their own authority allows it.

## Inspect registered agents

Use a company administrator credential or administrator login session with
`credential_admin`:

```bash
curl --get "$QILBEEDB_URL/api/v1/agents" \
  --header "Authorization: Bearer $QILBEEDB_COMPANY_ADMIN_TOKEN" \
  --data-urlencode 'contract_version=1' \
  --data-urlencode 'limit=25'
```

The directory derives the company from the authenticated administrator. It does
not accept a caller-selected company or allow ordinary integration credentials
to enumerate the company. The global administrator uses the existing explicit
company administration flow to obtain authority in the selected company.

Example response, with synthetic identifiers:

```json
{
  "contract_version": 1,
  "company_id": "example-company",
  "page": {
    "agents": [
      {
        "agent_id": "consumer-agent-928",
        "registered_at_millis": 1700000000123,
        "registered_by": {
          "credential_id": "3a13832a-a324-48a5-b6e3-bd453c95c201",
          "subject_id": "company-agent-system"
        },
        "first_scope": {
          "project_id": "support",
          "agent_id": "consumer-agent-928",
          "mission_id": null,
          "visibility": "private"
        },
        "private_subject_id": "company-agent-system",
        "trigger": {
          "kind": "memory_command",
          "record_id": "eb3c7e52-7944-4677-9e5d-5a7253f99252",
          "revision": 1,
          "action": "created"
        }
      }
    ],
    "next_after_agent_id": null
  }
}
```

| Field | Meaning |
| --- | --- |
| `agent_id` | The unchanged external ID, unique within this company |
| `registered_at_millis` | Server time when the association was committed |
| `registered_by` | Credential ID and subject making the first observed request |
| `first_scope` | The project, agent, mission, and visibility of that request |
| `private_subject_id` | The first request's private subject; `null` for shared scope |
| `trigger.kind = memory_command` | Includes the exact command record, revision, and action |
| `trigger.kind = successful_resource_request` | Successful scoped operation without a memory-command receipt |
| `next_after_agent_id` | Exclusive continuation ID; `null` when this traversal has no further observed row |

The association is immutable. Later requests, another project, a different
permitted credential, or credential rotation do not replace the first record.
Under concurrency, the first durable registration wins; arrival time at the
network boundary does not define the winner. Existing registration never skips
authorization or turns a denied operation into a success.

The directory exposes first-observation metadata, not active agent status,
heartbeat, current project membership, business ownership, or a complete list of
workspaces and memories. Use the application's own lifecycle system for those
business decisions.

## Continue a directory traversal

`limit` defaults to 25 and accepts 1–100. When `next_after_agent_id` is non-null,
supply its exact value as the URL-encoded `after_agent_id` query parameter:

```bash
curl --get "$QILBEEDB_URL/api/v1/agents" \
  --header "Authorization: Bearer $QILBEEDB_COMPANY_ADMIN_TOKEN" \
  --data-urlencode 'contract_version=1' \
  --data-urlencode 'limit=25' \
  --data-urlencode "after_agent_id=$NEXT_AFTER_AGENT_ID"
```

Storage selects the company prefix before traversal and orders IDs by their
UTF-8 bytes. The cursor is an external ID, not an offset or an opaque encoded
snapshot. It is exclusive: a continuation does not repeat its boundary row.
Pages are live. Concurrent registrations sorting before the cursor require a new
traversal; registrations after it may appear in a continuation. No total count or
snapshot completeness is claimed. Each request visits at most the requested page,
a boundary row if present, and one continuation lookahead within the company.

## Failures and retry behavior

| Status | Directory behavior |
| --- | --- |
| 200 | Returns the company page, including an empty page |
| 400 | Invalid contract version, limit, cursor ID, or unknown query field |
| 401 | Missing, expired, revoked, or otherwise invalid authentication |
| 403 | Authenticated caller lacks company administration |
| 500 | Storage failure or inconsistent stored registration; no partial page is returned |

Response caching is disabled with `Cache-Control: no-store`. Internal storage
namespaces and credential secrets are never included in directory responses.

For memory commands, retry the identical command with the same idempotency key
when the response is lost. An acknowledged command from before this feature can
create its first post-upgrade association on a valid replay, without creating a
new memory or changing its original receipt. A conflicting replay does not
register the agent. This registration time denotes the post-upgrade observation,
not the time of the original command.

Other scoped operations register after their operation succeeds and before the
HTTP handler returns. They may use a different store or an earlier independent
write. If registration then fails, the API returns an error even though that
operation may already have committed. Reconcile using that operation's existing
receipt, revision, or read contract; do not assume an error means no side effect.
This feature does not add a transaction across identity, learning, and memory
stores. Concurrent revocation retains the existing behavior: a request already
authorized may finish. A lost HTTP response is not proof of rollback.

## Upgrade, retention, and validation

Collection begins when this feature runs. It does not scan old memories or infer
historical agent IDs from credential names. Historical agents appear after their
next successful authorized request. Existing memory and authorization namespaces
remain unchanged. The trusted Rust storage API only registers when its caller
supplies an `AgentObservation`; the server derives that observation from verified
authorization. Direct library callers remain responsible for authentication and
for binding company, ID, subject, and namespace correctly.

Deleting memories, expiring credentials, changing grants, and revoking access do
not erase the immutable registration. It is retained first-observation metadata;
this release has no registration removal, rename, or agent suspension endpoint.
It does not reactivate credentials or authorize reads of deleted or private data.

Older binaries do not collect new observations. A mixed-version interval leaves a
directory coverage gap until those agents make successful requests on a supporting
version. Follow the stricter [credential-policy downgrade requirements](company-integrations.md#upgrade-and-qualification)
when policies or authority-change records have also been used.

Qualification covers real TCP responses against the served OpenAPI, empty reads,
unchanged Unicode IDs, same-ID company isolation, capability and scope denial,
revocation, invalid queries, bounded continuation, concurrent first writes,
receipt replay, invalid stored metadata, and failed batches. A subprocess test
kills the server after acknowledged memory and read registrations and verifies
both associations and the original memory receipt after restart. These checks
validate the registration contract; they do not establish retrieval relevance or
improved agent reasoning.

The 0.12.0 [company memory inventory](company-memory-administration.md) is a
separate directory derived from retained memory storage. Its startup migration
includes pre-existing private workspaces without fabricating historical agent
registration events.
