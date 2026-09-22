# Learned Tool API

The `/api/v1/tools` API stores immutable artifacts, registers external executor
profiles and records development/repair outcomes. A client can use ordinary HTTPS
JSON requests without a compiler, model or local execution runtime. The database
stores source and evidence; separately deployed workers generate and test code.
These routes do not dispatch programs or publish executable releases.

Use `http://localhost:7474` for local Docker. The interactive reference is
[`/docs`](http://localhost:7474/docs); [OpenAPI](openapi.json) describes every
request and response. Requests require `contract_version: 1`, reject unknown
fields and share the 65536-byte transport limit. Successful commands and reads
return **200**, including non-successful worker outcomes. Check the returned
`state` and `reason`, not only HTTP status. Responses carry `Cache-Control: no-store`.

## Ownership and compatibility

These existing routes document artifact and development records; they do not
assign ownership of tool code or execution to QilbeeDB. Applications retain that
responsibility. New knowledge integrations should follow the
[external tool knowledge boundary](../architecture/learned-tools.md), without
requiring code or executor registration. No endpoint removal or data migration
is introduced by this clarification.

## Credentials and routes

Use `Authorization: Bearer <credential>`. The credential administrator issues
separate credentials with explicit capabilities. Existing memory or procedural
capabilities do not grant tool access. Bootstrap administrators can issue a
`tool_admin` credential; bootstrap does not implicitly include that capability.
Tenant and actor are derived from current credentials, never from request JSON.

| Endpoint | Capability and additional authority | Response |
| --- | --- | --- |
| `POST /api/v1/tools/executors` | `tool_admin` | `executor` |
| `GET /api/v1/tools/executors/{id}` | `tool_admin` | `executor` |
| `POST /api/v1/tools/artifacts` | `tool_develop`, exact scope | `artifact` |
| `POST /api/v1/tools/artifacts/read` | `tool_read`, exact scope | `artifact` |
| `POST /api/v1/tools/development/requests` | `tool_develop`, exact scope | original `receipt` |
| `POST /api/v1/tools/development/read` | `tool_read`, exact scope | current `development` |
| `POST /api/v1/tools/development/commands` with `report` | `tool_report`, exact scope, registered executor subject | original `event` |
| Same endpoint with `request_cancellation` | `tool_develop`, exact scope, original requesting subject | original `event` |
| `POST /api/v1/tools/development/events/read` | `tool_read`, exact scope | original `event` |

Each response is an envelope with `contract_version: 1` and the named field.
Shared access requires the same exact grant in the same tenant. Private scope
includes the authenticated subject, so a separate worker subject cannot report
on another subject's private request. Use a shared scope with explicit grants for
separate developer/worker identities. Revoked, rotated-old or expired secrets
cannot authorize a later request. An operation already authorized may finish
while revocation races with it.

## Register an executor and request development

An administrator posts this synthetic profile to `/api/v1/tools/executors`:

```json
{
  "contract_version": 1,
  "profile": {
    "id": "worker-v1",
    "subject_id": "worker",
    "runtime_image_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "environment_revision": "sandbox-v1",
    "permissions_revision": "no-network-v1",
    "max_cost_units": 100,
    "max_latency_ms": 60000
  }
}
```

Use a real immutable image digest in integration. Preserve the returned
`executor.profile_digest`. Profile IDs are immutable within the tenant; changed
content requires a new ID. Profiles contain no endpoints or worker passwords.
They declare expected authority and execution context, not a sandbox attestation.
Cost units must have a consistent deployment-defined meaning.

A developer posts this to `/api/v1/tools/development/requests`:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "request": {
    "id": "request-v1",
    "executor_id": "worker-v1",
    "objective": "Develop an identity tool and run its tests.",
    "parent_artifact_id": null,
    "repair_evidence_ref": null
  }
}
```

The receipt preserves the immutable request, request digest, executor profile
digest, tenant/namespace, requesting actor and timestamp. Repeating the same
request ID/content by the same subject returns the original receipt, even after
completion. Changed content or a different requester conflicts. For a repair,
both parent and evidence are required; the parent must exist in the same scope.
External evidence references are preserved as declarations, without fetching them.

Creation records intent. It does not dispatch work. A configured worker is given
the request ID and shared scope and reads the current state at
`/api/v1/tools/development/read`:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "request_id": "request-v1"
}
```

## Report a candidate or failure

Post to `/api/v1/tools/development/commands` with the worker credential. Replace
the zero-filled digest with the exact registered `executor_profile_digest`:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "request_id": "request-v1",
  "command": {
    "event_id": "tests-completed-v1",
    "expected_revision": 1,
    "action": {
      "type": "report",
      "report": {
        "executor_profile_digest": "0000000000000000000000000000000000000000000000000000000000000000",
        "outcome": "succeeded",
        "evidence_ref": "test-run-v1",
        "detail": "Identity cases passed in the configured worker.",
        "cost_units": 10,
        "latency_ms": 100,
        "artifact": {
          "id": "identity-v1",
          "source": "def run(value):\n    return value\n",
          "dependency_lock": "",
          "runtime_image_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "entrypoint": "tool:run",
          "input_schema": true,
          "output_schema": true,
          "source_refs": ["development:request-v1"],
          "parent_artifact_id": null,
          "repair_evidence_ref": null
        }
      }
    }
  }
}
```

This is a payload example, not measured tool efficacy. Successful reports must
match the bound runtime image, parent/evidence and `development:<request ID>`
source reference. The artifact and resulting state are committed atomically with
the event. Changed event retries conflict; identical retries return the original
event snapshot and actor. Credential rotation is compatible when subject and
grants stay the same. New commands require the latest revision.

For a failed or pending report, use outcome `failed` or `pending_or_unknown`, set
`artifact` to null, and preserve a diagnostic `evidence_ref`/`detail`. Unknown
accounting is null. A reported success with unknown accounting remains pending;
a known budget violation becomes failure. Known cumulative accounting cannot
decrease, including across unknown reports. `observed_cost_units` and
`observed_latency_ms` retain known lower bounds while current consumption may be
unknown. See the complete [state contract](../agent-memory/tool-development.md).

## Cancellation and immutable event reads

The original requester can ask to cancel a nonterminal request:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "request_id": "request-v1",
  "command": {
    "event_id": "cancel-v1",
    "expected_revision": 1,
    "action": {"type":"request_cancellation","reason":"The mission ended."}
  }
}
```

State becomes `cancellation_requested`. Only a bound worker report with outcome
`cancelled` confirms cancellation. A complete success can win the race before
that confirmation. New commands after a terminal state conflict; old identical
receipts remain replayable. No automatic redispatch occurs on timeout or restart.

Read an original receipt at `/api/v1/tools/development/events/read`:

```json
{
  "contract_version": 1,
  "scope": {"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"},
  "request_id": "request-v1",
  "event_id": "cancel-v1"
}
```

## Direct artifacts and response fields

To import source without claiming development success, post
`{contract_version, scope, artifact}` to `/api/v1/tools/artifacts`, using the same
artifact shape as the worker example. Read it with
`{contract_version, scope, artifact_id}` at `/api/v1/tools/artifacts/read`.

Artifacts retain `proposal`, `artifact_digest`, exact source/dependency SHA-256
digests, actor, timestamp, tenant, namespace and schema version. Executor records
retain `profile`, `profile_digest` and administrative provenance. Development
state retains its original `receipt`, `revision`, `state`, `reason`,
`last_event_id`, optional exact artifact ID/digest and nullable accounting.
Events retain the complete `command`, reporting `actor`, resulting `record`,
schema version and timestamp. An old event's snapshot may differ from current
state; read the development record for the latest state.

The [artifact contract](../agent-memory/tool-artifacts.md) lists byte limits and
digest semantics. Objectives/details support 8192 bytes; evidence and cancellation
reasons support 2048 bytes. Schemas are stored declarations, not compiled validators.
Source, lock files and evidence must contain no credentials. A successful
artifact is not automatically published or authorized for invocation.

## Errors

| Status/code | Meaning |
| --- | --- |
| 400 `invalid_request` | Unknown fields/version, invalid bounds or malformed report |
| 401 `unauthorized` | Invalid, expired, revoked or replaced bearer secret |
| 403 `forbidden` | Missing capability/grant or wrong requesting/executor subject |
| 404 `record_not_found` | Request, artifact, event or executor absent in authorized scope/tenant |
| 409 `revision_conflict` | A new command used a stale revision |
| 409 `idempotency_conflict` | Changed immutable content, mismatched runtime/provenance/profile, or invalid terminal transition |
| 413 `invalid_request` | Complete request body exceeds 65536 bytes |
| 500 `storage_inconsistency` | Stored identity, digest, state/event or artifact mismatch |

Errors use `{contract_version, error: {code, message}}`. On an ambiguous transport
failure, replay the identical request/event ID. Do not invent a new operation ID
that could cause duplicate external work. The database's receipts coordinate
worker evidence; workers still need their own durable execution identities,
isolation and reconciliation protocol.
