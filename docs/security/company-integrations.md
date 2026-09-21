# Company integration scope policies

**Availability:** implemented in the unreleased company-integration feature.
The deployed 0.11.0 API does not accept `scope_policy` or the scope-authority
endpoint. Check the installation's published OpenAPI before using this contract.

A company integration can use externally managed project, agent, and mission IDs
without issuing a credential or adding an exact grant for every new ID. A company
administrator explicitly selects a versioned scope policy for that integration.
The application owns ID generation, membership, and resource lifecycle. IDs are
opaque, case-sensitive UTF-8 strings; QilbeeDB does not normalize them or infer
identity from display names.

This feature authorizes dynamic scopes. Durable first-request agent registration,
delegated child-key issuance, and a storage-backed company memory inventory are
separate capabilities; this policy does not claim to implement them.

## Choose an access model

| Credential configuration | Effective resource authority |
| --- | --- |
| `grants` with no policy, or `scope_policy: null` | Exact project, agent, mission, and visibility tuples |
| `grants: []` with no policy | No data scopes, including on existing credentials |
| `grants: []` with `company_scopes_v1` | Scopes satisfying every selector and visibility rule |
| Nonempty grants together with a policy | Rejected as ambiguous configuration |

Capabilities remain independent. A matching policy with `memory_read` does not
permit a memory write, human review, procedure evaluation, or credential issuance.
Company integrations cannot contain `credential_admin`, because that capability
could issue a less restricted key. Use a separate company administrator for access
administration. Tenant-level capabilities such as `policy_admin` and `tool_admin`
retain their documented company-level behavior; project selectors do not turn
those operations into project-specific administrative roles.

The tenant comes from the authenticating credential. A policy cannot name another
company. Private memory continues to include the credential's subject in its
namespace; covering all agents does not change private ownership. Preserve the
established subject when migrating an integration so existing private memory
remains addressable. Company administrators can explicitly delegate access to
company subjects; ordinary integrations cannot supply an arbitrary owner in a
memory request.

## Issue an integration key

Send `POST /api/v1/credentials` with a company administrator bearer token:

```json
{
  "contract_version": 1,
  "spec": {
    "subject_id": "company-agent-system",
    "capabilities": ["memory_read", "memory_write"],
    "grants": [],
    "scope_policy": {
      "version": "company_scopes_v1",
      "projects": {"mode": "all"},
      "agents": {"mode": "all"},
      "missions": {"mode": "all"},
      "allow_unassigned_mission": true,
      "visibilities": ["shared", "private"]
    },
    "expires_at_millis": null
  }
}
```

HTTP 201 returns sanitized credential metadata and a one-time secret. Configure
expiry according to company policy; `null` means no configured expiry. Store the
secret in the consuming system's secret store. Issuance retains its existing
one-time delivery semantics: if the response is lost, inspect and rotate through
a surviving administrator instead of repeatedly issuing keys without recovery.

Use the integration key on `POST /api/v1/memory/commands` with a new external ID:

```json
{
  "contract_version": 1,
  "idempotency_key": "observation-0001",
  "scope": {
    "project_id": "consumer-project",
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
      "metadata": {"source": "consumer-run-0001"}
    }
  }
}
```

The receipt identifies the durable memory revision. An embedding is not required
for this operation. Existing memory idempotency and revision rules apply. The
same key can handle another external agent ID if its selectors permit it.

## Restrict the integration

Every policy field is required. There are no implicit wildcard defaults.

| Field | Contract |
| --- | --- |
| `version` | Exactly `company_scopes_v1`; unknown versions are rejected |
| `projects` | `{"mode":"all"}` or `{"mode":"only","ids":[...]}` |
| `agents` | The same explicit selector form for externally supplied agent IDs |
| `missions` | Selects assigned mission IDs only |
| `allow_unassigned_mission` | Independently permits or denies a missing/null mission |
| `visibilities` | One or two distinct values from `shared` and `private` |

An empty `only` list permits no IDs; it never means all. Each list accepts at most
256 distinct IDs, each with 1–256 UTF-8 bytes, no control characters, and at least
one non-whitespace character. These are representation/work limits, not a quota
on agents covered by `all`. The complete body remains bounded by 65,536 bytes.
Duplicate values, extra selector fields, and missing policy fields fail validation.

For example, one project, any agent in it, and no assigned missions:

```json
{
  "version": "company_scopes_v1",
  "projects": {"mode": "only", "ids": ["support"]},
  "agents": {"mode": "all"},
  "missions": {"mode": "only", "ids": []},
  "allow_unassigned_mission": true,
  "visibilities": ["shared"]
}
```

## Change access without replacing the secret

Use `POST /api/v1/credentials/{id}/scope-authority` with a company administrator
token. First inspect the target credential to obtain its current revision.

```json
{
  "contract_version": 1,
  "expected_revision": 1,
  "authority": {
    "grants": [],
    "scope_policy": {
      "version": "company_scopes_v1",
      "projects": {"mode": "only", "ids": ["support"]},
      "agents": {"mode": "all"},
      "missions": {"mode": "only", "ids": []},
      "allow_unassigned_mission": true,
      "visibilities": ["shared"]
    }
  }
}
```

HTTP 200 returns `contract_version` and `credential`, without a secret. This
replaces the complete scope authority; it does not merge policies. Tenant,
subject, secret, capabilities, and expiry remain unchanged. To restore exact
grants, provide them and omit the policy. To suspend resource access while
retaining authentication, use `{"grants":[]}`. Tenant-level capabilities remain
active; revoke the credential to disable all authentication.

Every success increments the revision and appends a `scope_authority_changed`
event with actor, time, and full `previous` and `current` scope authorities.
The target and administrator revision guards, new authority, and audit event
commit in one synchronous WAL-backed metadata batch. Eight competing updates at
one expected revision produce one success and seven conflicts.

| Status | Meaning and handling |
| --- | --- |
| 200 | Scope authority committed; retain the returned revision |
| 400 | Invalid authority/version/ID, expired target, or invalid revision representation |
| 401 | Calling credential/session is absent, expired, revoked, rotated, or otherwise invalid |
| 403 | Caller lacks company administration, target belongs to another company, or target is revoked |
| 409 | Target or authorizing authority changed; inspect before deciding a new update |
| 413 | Request exceeds the transport body bound |
| 500 | Storage or integrity failure; no success is implied |

If the response is lost, inspect the credential and compare its event and revision.
Replaying a stale revision returns 409 and cannot overwrite a later change. This
is revision-based reconciliation, not an idempotent secret-delivery protocol.

The API key remains usable under its new limits. Login sessions bound to the
changed credential become invalid because they pin its old revision; sign in
again. New authorization checks read durable state. Requests already authorized
may complete; this contract does not cancel in-flight work or provide a
cross-database transaction with memory writes.

## Upgrade and qualification

Existing credential records omit the optional policy and audit fields and retain
their previous wire representation. Exact-grant namespaces are identical to
policy-authorized namespaces for the same company, scope, and private subject.
Changing access does not move or delete memories.

An older binary rejects credentials containing the new fields. After enabling a
policy or recording a scope-authority change, do not roll back only the binary.
Use a compatible binary or an explicitly reconciled pre-upgrade backup. Restoring
a backup requires reconciling newer memories, credentials, and revocations.

Qualification covers real HTTP responses against the served OpenAPI, new external
IDs, same-ID company separation, shared/private subject boundaries, legacy empty
grants, negative selectors, stale writes, concurrent updates, login-session
invalidation, rotation/revocation, expiry, reopen, and inconsistent policy history.
A subprocess test kills the server after acknowledged policy and memory writes,
then verifies the policy, receipt replay, scope denial, and a later revocation
after restart. This is process-interruption evidence, not hardware power-loss testing.
Authorization design follows [OWASP request-level checks](https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html)
and [object-level authorization guidance](https://api-security.owasp.org/editions/2023/en/0xa1-broken-object-level-authorization/).
These references guide implementation; they are not a security certification or
evidence of improved agent reasoning.
