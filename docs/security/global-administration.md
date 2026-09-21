# Global administration and SaaS provisioning

QilbeeDB separates installation administration from company data access. A
locally provisioned **master** owns global administrative authority. A SaaS
backend can receive a narrower global credential to register companies. Each
company then uses its own tenant credentials and exact resource grants.

This contract is available in **0.11.0**. There is no default master password,
anonymous registration endpoint, or HTTP operation that creates the first master.

## Choose an authority

| Principal | Credential | Authority |
| --- | --- | --- |
| Installation master | `qdbg1_…`, `is_master: true` | All four global administrative capabilities; delegates service keys and appoints administrators in any tenant |
| Signup backend | `qdbg1_…`, `tenant_create` only | Creates a new tenant and its first administrator; cannot take over an existing tenant or administer global credentials |
| Administrative service | `qdbg1_…`, explicit subset | Performs only the selected global operations |
| Company administrator | `qdb1_…` | Issues credentials and manages policy inside its own tenant |
| Application or agent | `qdb1_…` | Uses explicitly granted capabilities and exact project, mission, agent and visibility scopes |

Global keys and tenant keys have separate persisted records, verifier domains and
authentication paths. A tenant cannot gain global authority by choosing a tenant
name, subject, capability string, grant or request field. Global keys do not
authenticate memory, search, learning or tenant credential endpoints.

The master has administrative control over every company: it can explicitly
appoint a tenant administrator, which can issue that company's application
credentials. This is powerful access and must be protected accordingly. The
appointment records the global actor in the new administrator's history; there
is no silent switch of a request's tenant or automatic wildcard memory grant.

## Bootstrap the master once

Stop the server and use exclusive access to its persistent data directory:

```sh
umask 077
(set -C; qilbeedb bootstrap-master /var/lib/qilbeedb/data master \
  > /secure/operator/master.json)
```

The command returns `contract_version`, `credential` and the one-time `secret`.
Store the result in a secret manager. Only a verifier is persisted in QilbeeDB;
the command's output must not enter application logs, Git, images or user data.
The command fails if this installation has ever bootstrapped a master, including
when that master was subsequently revoked. Starting an empty server alone does
not provision any authority.

Host administrators who can stop QilbeeDB and write its database files are trusted
operators. Protect host access, backups, container administration and secret
manager permissions as part of the same security boundary.

## Delegate the SaaS registration service

The master issues a backend key through `POST /api/v1/admin/credentials`:

```json
{
  "contract_version": 1,
  "spec": {
    "subject_id": "saas-signup-service",
    "capabilities": ["tenant_create"],
    "expires_at_millis": null
  }
}
```

Set an explicit future expiration for a time-limited service key. A global issuer
needs `global_credential_admin`, may delegate only capabilities it already has,
and cannot issue a credential whose expiry exceeds its own. Issued service keys
always have `is_master: false`; the payload cannot change that flag.

The response is HTTP 201 with sanitized `credential` metadata and the new
`secret`. Deliver the key only to the signup backend's secret store. Rotation,
issuance and initial administrator delivery return secrets once and are not
idempotent secret-delivery protocols. Persist the returned IDs and metadata.

The public signup flow should work as follows:

1. The browser authenticates to the SaaS backend using the SaaS application's
   identity provider. No global QilbeeDB key is sent to the browser.
2. The backend verifies the account, applies its registration policy and abuse
   limits, and generates an immutable company ID. It associates that ID with the
   verified account on the server; it does not trust an arbitrary tenant ID from
   browser storage or a query parameter as proof of membership.
3. The backend registers the company using its restricted global key.
4. It stores the returned tenant administrator key securely and uses it to issue
   appropriately scoped application credentials. It does not expose that
   administrator key as an ordinary user session.
5. Requests from agents and users use company-specific credentials. QilbeeDB
   checks tenant, capability, scope, subject and current credential state.

QilbeeDB supplies database authorization. Email verification, login sessions,
billing, invitation acceptance, signup rate limits and account recovery for the
SaaS application belong to the application layer. Possession of a
`tenant_create` key allows tenant creation, so it must never be distributed as a
public client API key.

## Register a company

Call `POST /api/v1/admin/tenants` with `Authorization: Bearer <global-key>`:

```json
{
  "contract_version": 1,
  "tenant_id": "company-6cb1eb2b",
  "subject_id": "initial-company-operator"
}
```

The server requires `tenant_create`. It atomically writes the tenant marker,
registration provenance and first tenant administrator credential while checking
the authorizing global credential's exact stored revision. The administrator
receives `credential_admin` and `policy_admin`, with no memory grants.

HTTP 201 contains `tenant`, `credential` and the one-time tenant `secret`.
`tenant.created_by` identifies the global actor; `initial_admin_id` identifies the
new administrator. Tenant and subject IDs contain 1–256 UTF-8 bytes, must not be
blank, and must not contain control characters. IDs are exact, case-sensitive
identities; they are not normalized company display names.

An existing tenant ID returns **409 `revision_conflict`** and never replaces its
authority. Concurrent requests for the same new ID have only one winner. This
also protects companies created earlier through `bootstrap-tenant`.

If the response is lost, do not claim the tenant was not created and do not retry
with a different company ID automatically. A credential with `tenant_inspect`
can inspect the registration; an operator with `tenant_admin` can appoint a new
administrator when secret delivery is uncertain. The registration-only service
cannot perform that recovery. Revoke an unaccounted-for initial administrator
through the tenant credential API after recovering access.

## Administrative API

All routes require a global bearer key, use `contract_version: 1`, and return
`Cache-Control: no-store`. The request body limit is 65,536 bytes.

| Method and path | Required authority | Result |
| --- | --- | --- |
| `GET /api/v1/admin/identity` | Any active global credential | Current credential metadata |
| `POST /api/v1/admin/credentials` | `global_credential_admin`, subset and expiry checks | New delegated global credential and one-time secret |
| `GET /api/v1/admin/credentials/{id}` | `global_credential_admin`, capability superset of target | Credential metadata and history |
| `POST /api/v1/admin/credentials/{id}/rotate` | Self, or authorized global credential administrator | New secret at the expected revision |
| `POST /api/v1/admin/credentials/{id}/revoke` | Self, or authorized global credential administrator | Revoked credential metadata |
| `POST /api/v1/admin/tenants` | `tenant_create` | New company and first administrator |
| `GET /api/v1/admin/tenants/{tenant}` | `tenant_inspect` | Tenant ID and registration provenance |
| `POST /api/v1/admin/tenants/{tenant}/admin-credentials` | `tenant_admin` | Additional administrator for that exact existing tenant |

Rotation and revocation use
`{"contract_version":1,"expected_revision":1}`. Delegated administrators cannot
rotate or revoke the bootstrap master, even if they hold all global capabilities.
A key can rotate or revoke itself. Rotation preserves permissions and expiry.
Each change appends actor, action, revision and timestamp to the target history.
Old and revoked secrets stop authenticating durably, including after restart.

Administrator appointment uses
`{"contract_version":1,"subject_id":"replacement-operator"}`. It does not
automatically revoke existing tenant keys. Inspection returns `registration:
null` for locally bootstrapped tenants, whose original registration provenance
must not be fabricated.

These capabilities are independent: `tenant_create` does not imply
`tenant_inspect`, `tenant_admin`, or `global_credential_admin`. Revoking an issuer
does not cascade to previously issued keys or delete companies. Revoke affected
credentials explicitly using their recorded IDs. This release does not expose a
tenant listing, company suspension, billing API or human login interface.

## Errors and recovery

| Status | Meaning |
| --- | --- |
| 400 | Invalid JSON, unknown fields, unsupported version or invalid identity/expiry |
| 401 | Missing, malformed, expired, rotated or revoked credential |
| 403 | A valid tenant key used for global administration, or a global key without required authority |
| 404 `tenant_not_found` | Authorized lookup or administrator appointment for an absent tenant |
| 409 `revision_conflict` | Existing tenant, stale expected revision or concurrent authority change |
| 413 | Body exceeds the transport limit |
| 500 | Internal failure or stored authority inconsistency; fail closed |

If the master key is lost or revoked, a trusted host operator can stop the server
and run `qilbeedb recover-master <data-directory> <expected-revision>`. This
rotates only the existing bootstrap master, preserves its ID and history, and
records `recovered_by_local_operator`. A stale revision fails. Secure the new
one-time output before restarting the service. There is no network recovery
endpoint and recovery does not revive revoked delegated keys.

## Validation and security basis

The feature's tests exercise separate global and tenant authentication, denied
privilege escalation, restricted signup keys, cross-company memory isolation,
concurrent registration, expiry, rotation, revocation after reopening storage,
offline recovery and corrupt authority records. A real loopback HTTP test checks
responses against the OpenAPI document served by the same process.

The design follows least privilege, default denial and authorization on every
request as described in the [OWASP Authorization Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html).
For AWS deployments, use the access control and rotation guidance in
[AWS Secrets Manager best practices](https://docs.aws.amazon.com/secretsmanager/latest/userguide/best-practices.html).
These implementation tests are not an external penetration test or a compliance
certification.
