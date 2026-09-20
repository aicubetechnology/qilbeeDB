# Durable scoped credentials

`qilbee_server::security::identity::IdentityStore` is the platform's persistent
credential authority. It uses the existing storage engine and has no QMN runtime,
network, model-provider or service dependency. This feature is a Rust API
foundation: **the legacy HTTP router does not yet use it**. Its old authentication
and resource-authorization limitations remain until the secure router is wired in.

## Identity and grants

A credential binds one tenant and one subject. Administrative issuance derives
the tenant from the authenticated administrator, never from an issuance payload.
The strict `CredentialSpec` decoder rejects extra fields such as `tenant_id`.
Subject and resource identifiers are explicit UTF-8 strings. Their 256-byte limit
is an encoding/transport bound, not an episode, tenant or learning quota.

Capabilities are independent; names do not imply other permissions:

| Capability | Intended operation |
| --- | --- |
| `memory_read` | Read or query an explicitly granted memory scope |
| `memory_write` | Mutate an explicitly granted memory scope |
| `procedure_propose` | Propose candidates in a granted scope |
| `procedure_evaluate` | Submit evaluations in a granted scope |
| `policy_admin` | Administer tenant policy through its future policy API |
| `credential_admin` | Issue, inspect, rotate and revoke tenant credentials |

A resource grant is an exact tuple of project, optional mission, agent and
visibility. Omitting a mission means the missionless scope; it is not a wildcard.
A private namespace includes the authenticated subject. A shared namespace can be
used by other subjects only with the same explicit grant in the same tenant.
JSON tuple encoding preserves component boundaries, including separator-like
characters inside identifiers. Two tenants with identical project and agent IDs
therefore address different storage namespaces.

An administrator does not implicitly receive memory read access. A proposing
credential does not implicitly receive evaluation, policy or credential powers.
`authorize` must be called for each resource operation; its returned namespace is
derived from the current credential rather than a caller-provided tenant or owner.

This follows the object-level checks described by
[OWASP API Security](https://api-security.owasp.org/editions/2023/en/0xa1-broken-object-level-authorization/)
and the explicit authorization rules in the
[OWASP Authorization Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html).
These references motivate the contract; they are not a certification.

## Operator bootstrap and lifecycle

`bootstrap_tenant(tenant, subject)` is an explicit **trusted local operator** API.
It creates a tenant marker and administrator credential atomically, exactly once.
The marker remains even after credential revocation. Opening a store does not
create an administrator, choose a known password, or reset an existing tenant.
Do not expose this method or raw storage as an anonymous HTTP operation.

Keys contain an opaque identifier and 256 random bits from the operating system.
Only a domain-separated HMAC-SHA256 verifier is persisted. Verification uses the
MAC library's constant-time comparison. The secret is returned once and is
redacted from the issued credential's `Debug` output. Administrative views omit
the verifier as well as the secret.

- `issue(admin_token, spec)` grants only the explicit capabilities and scopes.
- `authenticate(token)` reads the current durable state and checks expiry,
  revocation and the secret verifier. It does not rely on cached JWT claims.
- `rotate(admin_token, credential_id, expected_revision)` replaces the secret
  without changing tenant, subject, grants, capabilities or expiry. The old secret
  stops authenticating at commit.
- `revoke(admin_token, credential_id, expected_revision)` records revocation.
  This API cannot reactivate a revoked credential.
- `inspect(admin_token, credential_id)` returns sanitized metadata and successful
  change history within the administrator's tenant.

Each successful change appends actor ID, action, time and revision to the same
record. Stale revision writes fail. The administrator's exact credential revision
is checked in the write batch, so an intervening rotation or revocation cannot be
ignored by a later credential mutation. Expiration is checked at authentication
time using the host wall clock. Unsupported record versions fail closed.

Issuance and rotation return new secret material once; they are not replayable
idempotent commands. If a response containing a new secret is lost, a surviving
administrator must inspect the revision and rotate again. There is no automatic
reset path. Successful change history is not a complete audit of failed attempts.

## Storage and integration contract

`StorageEngine::compare_and_write_meta` checks exact previous values and commits
all guarded metadata mutations in one RocksDB batch. It always enables WAL and
synchronous writes, even when bulk graph settings choose weaker durability. A
failed condition writes nothing. Every modified key must have exactly one
condition, and duplicate conditions or writes are rejected. Read-only conditions
can guard the authorizing credential. Engine clones share writer serialization;
ordinary metadata writes participate in the same lock.

This does not add snapshots or conflict detection to every graph operation.
Credential methods perform blocking storage work and must be offloaded by async
transport adapters. Trusted Rust embedders and filesystem operators retain raw
storage authority. Serving these contracts over HTTP, protecting legacy routes,
and managing policy are subsequent features, not implicit effects of defining
capability names.

## Validation

Tests cover reopen without persisting secrets, exact scope and capability checks,
same-ID cross-tenant isolation, private versus shared subjects, expiry, malformed
and non-ASCII tokens, unknown schema versions, administrator self-rotation,
revocation, and strict issuance payloads. Concurrent bootstrap and rotation each
produce exactly one winner. Storage tests verify all-or-nothing batches, stale
conditions, deletion, reopen, duplicate guards and concurrent writers.

The tests use temporary local databases and require no QMN processes or model
calls. The process-crash test for HTTP memory remains in the workspace suite;
credential-specific hardware power-loss and operational recovery drills are not
established by these tests.
