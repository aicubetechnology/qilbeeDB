# Platform HTTP API

Rust **0.2.0** makes the authenticated platform router the default. There is no
built-in administrator password, shared JWT signing secret, anonymous graph
route, or HTTP tenant-bootstrap endpoint in this router. Every operation except
`GET /health` requires a durable platform bearer credential.

This is an intentional compatibility change from the 0.1 legacy HTTP surface.
The current increment exposes identity and credential administration. Scoped,
versioned memory CRUD and procedural operations are the next platform contracts;
legacy memory routes are not silently mapped into tenant namespaces.

## Provision a tenant locally

Stop any server using the data directory, then run the explicit operator command:

```sh
cargo run -p qilbee-server --bin qilbeedb -- \
  bootstrap-tenant ./data example-company initial-operator
```

The command writes a single JSON result to standard output containing
`contract_version`, sanitized `credential` metadata, and the new `secret`.
Deliver that secret to the intended operator securely. It is not a fixed password
or an application log entry, and only its verifier is stored in the database.
Bootstrap never resets an existing tenant, even if its administrator was revoked.
Opening the database and starting the server do not bootstrap implicitly.

Start the default server with the same directory:

```sh
cargo run -p qilbee-server --bin qilbeedb -- ./data
```

Use the returned secret as `Authorization: Bearer <secret>`. Platform credentials
are opaque `qdb1_...` keys; legacy JWTs and API keys do not authenticate here.
Use TLS termination for network deployment; this change does not add a TLS listener.

## Identity and credential endpoints

All JSON responses use `contract_version: 1`. Authenticated responses containing
credentials, as well as errors, send `Cache-Control: no-store`.

| Method and path | Permission and response |
| --- | --- |
| `GET /health` | Public status and server version |
| `GET /api/v1/identity` | Current credential metadata, without secret/verifier |
| `POST /api/v1/credentials` | `credential_admin`; create inside the caller's tenant |
| `GET /api/v1/credentials/{id}` | `credential_admin`; inspect a credential in the same tenant |
| `POST /api/v1/credentials/{id}/rotate` | `credential_admin`; replace the secret at an expected revision |
| `POST /api/v1/credentials/{id}/revoke` | `credential_admin`; revoke at an expected revision |

For issuance:

```json
{
  "contract_version": 1,
  "spec": {
    "subject_id": "research-agent-user",
    "capabilities": ["memory_read", "procedure_propose"],
    "grants": [{
      "project_id": "research",
      "mission_id": "experiment-001",
      "agent_id": "research-agent",
      "visibility": "shared"
    }],
    "expires_at_millis": null
  }
}
```

Issuance returns HTTP 201 with `credential` and one-time `secret`. The request
cannot override tenant identity. Explicit capabilities do not inherit evaluation
or policy powers. Bootstrap administrators have credential and policy
administration capabilities, but no implicit memory grants.

Rotation and revocation require:

```json
{"contract_version": 1, "expected_revision": 1}
```

Rotation returns the new secret once. Revocation returns sanitized metadata.
Revision mismatch returns 409 and leaves the credential unchanged. Both operations
check the authorizing credential and target tenant. Authentication reads durable
state on every request; old or revoked secrets fail after restart too.

Issuance and rotation are not idempotent secret-delivery protocols. If the result
is lost, use a surviving administrator to inspect state and rotate again. The
full [credential contract](../security/scoped-credentials.md) describes scope
encoding, expiry, change history and operator authority.

## Errors and transport limits

Errors have this envelope:

```json
{"contract_version":1,"error":{"code":"unauthorized","message":"A valid platform bearer credential is required"}}
```

The router distinguishes unauthenticated credentials (401), missing authority
(403), unknown routes (404), unsupported methods (405), invalid request/version
(400), stale revisions (409), oversized bodies (413), and internal failure (500).
Internal storage errors do not expose paths or credentials. Duplicate
Authorization headers are rejected. Strict JSON contracts reject extra fields.

Credential request bodies are limited to **65,536 bytes**. This is a transport
bound, not a business quota or token count. Storage and verification operations
run on Tokio's blocking pool, outside asynchronous request workers.

## Legacy compatibility

Applications that deliberately need the old surface can explicitly call
`http_server::create_legacy_router(database)` or set
`ServerConfig::enable_legacy_http = true` when embedding the server. The default
is false in development and production. Legacy mode uses a separate router; it
does not compose unprotected routes into the platform router.

Legacy mode retains known fixed-bootstrap and resource-authorization limitations.
Its authentication, graph endpoints, synthetic semantic scores and global agent
namespaces do not acquire platform guarantees. The legacy `auth_enabled` setting
controls only legacy bootstrap; setting it to false never disables platform auth.
There is no automatic tenant ownership inference or migration of legacy records.

## Validation

HTTP contract tests cover anonymous access, absent legacy routes, authenticated
issuance, strict tenant binding, capability denial, cross-tenant denial, expected
revision conflicts, rotation and revocation across restart, body limits, duplicate
authorization headers and non-cacheable responses. Operator tests cover explicit
arguments, one-time bootstrap and authentication after reopening the database.

These tests run without QMN, model providers or permanent auxiliary services.
They validate the implemented contracts, not completion of all P0 requirements.
