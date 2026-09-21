# Platform HTTP API

Rust **0.2.0** makes the authenticated platform router the default. There is no
built-in administrator password, shared JWT signing secret, anonymous graph
route, or HTTP tenant-bootstrap endpoint in this router. The API reference (`/`, `/docs`, `/docs/reference.js`), `/health` and
`/openapi.json` are public. All other operations require a durable platform bearer
credential.

This is an intentional compatibility change from the 0.1 legacy HTTP surface.
The platform exposes identity, credential administration,
[versioned scoped memory](versioned-memory.md),
[semantic vector retrieval](semantic-memory.md),
[lexical retrieval](lexical-memory.md),
[experimental hybrid retrieval](hybrid-memory.md),
[procedural learning](procedural-learning.md) and
[learned-tool development](learned-tools.md). Legacy memory routes are not
silently mapped into tenant namespaces.

For a multi-company installation, bootstrap the separate [installation master](../security/global-administration.md) and delegate a registration-only service key. Global `qdbg1_` keys authenticate only administration routes; tenant `qdb1_` keys remain company-bound.

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

Version 1 routes use `contract_version: 1`; the [verified change feed](verified-memory-changes.md) uses an explicit version 2 route and response. Authenticated responses containing
credentials, as well as errors, send `Cache-Control: no-store`.

| Method and path | Permission and response |
| --- | --- |
| `GET /health` | Public status and server version |
| `GET /openapi.json` | Public OpenAPI 3.1 contract for implemented endpoints |
| `GET /api/v1/identity` | Current credential metadata, without secret/verifier |
| `POST /api/v1/credentials` | `credential_admin`; create inside the caller's tenant |
| `GET /api/v1/credentials/{id}` | `credential_admin`; inspect a credential in the same tenant |
| `POST /api/v1/credentials/{id}/rotate` | `credential_admin`; replace the secret at an expected revision |
| `POST /api/v1/credentials/{id}/revoke` | `credential_admin`; revoke at an expected revision |
| `GET /api/v1/company/memory/workspaces` | Unreleased: `credential_admin`; discover all retained company memory workspaces |
| `POST /api/v1/company/memory/query` | Unreleased: `credential_admin`; inspect retained or eligible company records with bounded continuation |
| `POST /api/v1/company/memory/read` | Unreleased: `credential_admin`; inspect one retained record and its eligibility |
| `GET /api/v1/agents` | Unreleased: `credential_admin`; page through automatically registered agents in the authenticated company |
| `POST /api/v1/credentials/{id}/scope-authority` | Unreleased: `credential_admin`; replace exact grants or a company integration policy with a before/after audit event |

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
administration capabilities, but no implicit agent memory grants. The 0.12.0
[company inventory](../security/company-memory-administration.md) explicitly
allows `credential_admin` to inspect all retained company workspaces and private
subjects through separate administrative routes.

The 0.12.0 [company integration policy](../security/company-integrations.md)
adds explicit authority over dynamically supplied external IDs. Empty grants on
existing credentials remain deny-by-default. Its revision-guarded scope-authority
endpoint can change grants or policy without replacing the credential secret.

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
(400), stale revisions (409), oversized bodies (413), unavailable retrieval capacity
(503), and internal failure (500).
Internal storage errors do not expose paths or credentials. Duplicate
Authorization headers are rejected. Strict JSON contracts reject extra fields.

The published error schema includes these operational outcomes. Branch on status
and `error.code`, not message text:

| Status | Code | Handling |
| --- | --- | --- |
| 400 | `embedding_dimension_limit` | Check the ranking catalog's operator dimension ceiling; zero is invalid. Do not resize or relabel a vector implicitly. |
| 400 | `retrieval_scan_limit` | Check the requested byte budget against the operator ceiling. This is a rejected request, not a successful partial search. |
| 503 | `retrieval_busy` | No retrieval started because all instance slots were occupied. Retry with bounded backoff and jitter within the caller's deadline. |
| 404 | `review_not_found` | The authorized review state or requested immutable review revision is absent. A record can exist without that review revision. |

Authentication and scope checks precede these capacity and review lookups. A busy
server still returns 401 for a revoked credential and 403 for an ungranted scope.
Retrieval overload is 503, not 429. Responses do not promise a `Retry-After` delay.
The lexical, semantic and hybrid 503 schemas accept only `retrieval_busy`; review
lookup 404 schemas accept only `review_not_found`. All use `Cache-Control: no-store`.

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

These tests run without model providers or permanent auxiliary services.
They validate the implemented contracts, not completion of all P0 requirements.

## Human administrator login

Provisioned accounts can authenticate to short-lived sessions without receiving
permanent API keys. See [Administrator accounts and login](../security/human-login.md)
for the global and tenant endpoints, password lifecycle, cooldowns and exact limits.

The 0.12.0 [memory evidence graph routes](memory-evidence-graph.md) provide
scoped and company-administrative ancestry reads, exact revision-bound edges and
explicit coverage under the shared retrieval admission limit.

The unreleased 0.13.0 [typed memory relation routes](typed-memory-relations.md)
add durable assertions, current eligible reads, reviewer inspection and immutable
history. Writers and reviewers have separate capabilities; relation changes use
idempotency and expected-revision checks.
