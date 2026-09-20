# HTTP REST API

QilbeeDB Rust 0.3.0 exposes the authenticated platform API for durable identity
and scoped, versioned memory, plus evidence-driven procedural learning.

## Base URL

```text
http://localhost:7474
```

For a container on the configured integration network, use
`http://qilbeedb-local:7474`. See [Docker deployment](../operations/docker.md).

## API contract

Open the browser reference at [`/docs`](http://localhost:7474/docs), or the server
root URL, which redirects there. The page supports endpoint/schema search and
loads only the public contract from the same server.

Download the machine-readable [OpenAPI 3.1 specification](openapi.json), or request
`GET /openapi.json` from a running server. It lists implemented endpoints only.
The documentation uses the same field names, enum values and error envelopes.

| API | Detailed reference |
| --- | --- |
| Tenant provisioning, authentication and credential lifecycle | [Platform HTTP API](platform-http.md) |
| Create, conditional update, read, query and delete | [Versioned Memory API](versioned-memory.md) |
| Administrative learning contracts, proposals, outcomes and selection | [Procedural Learning API](procedural-learning.md) |
| Identity, scope, capability and secret-storage semantics | [Durable Scoped Credentials](../security/scoped-credentials.md) |
| Legacy endpoint persistence and limits | [Legacy Durable HTTP Memory](durable-http-memory.md) |

## Authentication

Provision a tenant through the local `bootstrap-tenant` operator command, then
issue credentials with the capabilities and exact resource grants needed by each
client. Use the returned opaque credential:

```bash
curl --fail 'http://localhost:7474/api/v1/identity' \
  --header "Authorization: Bearer $QILBEE_TOKEN"
```

The tenant and private subject come from authentication. Request bodies cannot
override them. Legacy Basic Auth, fixed administrator passwords and JWTs are not
credentials for this router. Health and the public OpenAPI document require no
credential; other implemented operations do.

## Health check

```bash
curl --fail 'http://localhost:7474/health'
```

Response:

```json
{"contract_version":1,"status":"healthy","version":"0.3.0"}
```

This is process health. It does not replace an authenticated write/read probe.

## Errors and retries

```json
{"contract_version":1,"error":{"code":"revision_conflict","message":"The expected revision or authorizing credential changed"}}
```

The API distinguishes invalid requests (400), authentication (401), authorization
(403), missing current records (404), unsupported methods (405), conflicting
commands/revisions (409), transport body limits (413), and internal or storage
consistency failures (500). Credential and memory responses use
`Cache-Control: no-store`.

For an uncertain memory mutation, retry the same command and idempotency key to
recover its original durable receipt. Changing the payload under that key is a
conflict. Credential issuance/rotation deliver a secret once and have different
recovery semantics, described in the platform reference.

## Compatibility and remaining APIs

The previous graph, login and global-agent HTTP routes are available only through
an explicitly opted-in legacy router with documented limitations. They are not
silently exposed by the default server or mapped into tenant scopes. Current
legacy SDK examples must not be assumed compatible with `/api/v1/memory`.

Shared changes feeds, indexed hybrid retrieval
and learned-tool services have acceptance criteria but no implemented endpoints
in this version. See the [delivery contract](../research/delivery-acceptance.md)
and [learned-tool architecture](../architecture/learned-tools.md).
