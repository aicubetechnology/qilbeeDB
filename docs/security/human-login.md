# Administrator accounts and login sessions

QilbeeDB supports explicitly provisioned username/password accounts in the 0.11.0
platform contract. A human account authenticates to a short-lived bearer session.
An account never receives its underlying permanent API key through login.

Use this contract for installation operators and company administrators. There
is no public signup, default password, email verification, password-reset email,
MFA, refresh token, or built-in login form. An application can provide its own
interface over these endpoints and keep its broader identity lifecycle separate.
All production authentication traffic must use HTTPS.

## Understand the authority boundary

| Account | Provisioning authority | Effective access |
| --- | --- | --- |
| Global operator | Installation master only | The bound global credential's capabilities |
| Company administrator | `credential_admin` in the same company | The bound tenant credential's capabilities and exact resource grants |
| Application user | Company administrator | Only the bound credential's explicit permissions |

A master account bound to the installation's bootstrap master can register
companies, appoint their administrators and administer global credentials. It
cannot use a global session directly on tenant memory endpoints. A company
account cannot create a global account, register another company or bind a
credential belonging to another company.

Each username is exact and case-sensitive within either the global authority or
one tenant. Identical usernames in different tenants are independent. Login
requires the tenant ID explicitly; the server verifies the account's stored
binding. No requested capability, tenant change or role is accepted by login.

## Create accounts

First bootstrap the installation master through the trusted local operator
command described in [Global administration](global-administration.md). Register
a company and retain its tenant administrator credential. These remain distinct
permanent secrets.

The master creates a global account with `POST /api/v1/admin/login-accounts`.
A company administrator uses `POST /api/v1/login-accounts` with a tenant bearer
credential. Both accept:

```json
{
  "contract_version": 1,
  "username": "operator@example.test",
  "password": "replace-with-a-private-password",
  "credential_id": "00000000-0000-0000-0000-000000000001"
}
```

Replace the example ID with the actual credential to bind. The global endpoint
requires `is_master: true`; a delegated service key does not gain account
administration merely by holding all four global capabilities. The tenant
endpoint derives its company from the authenticated administrator and rejects a
credential from any other tenant.

HTTP 201 returns `contract_version` and a sanitized `account`, including its ID,
username, authority, underlying credential ID, revision, disabled timestamp and
administrative history. Duplicate names return 409 without replacing the existing
account. This operation does not return or rotate an API key.

Passwords accept 8–1,024 UTF-8 bytes and are never truncated or normalized. Use
long, unique passwords; the lower bound is a compatibility limit, not a password
strength assessment. QilbeeDB stores only an Argon2id v19 verifier with a random
salt, 19 MiB memory, two iterations and one lane. These parameters follow the
[OWASP password storage guidance](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html).
The password and verifier are absent from public account views and audit history.

## Sign in

For a company account, send `POST /api/v1/login`:

```json
{
  "contract_version": 1,
  "tenant_id": "example-company",
  "username": "operator@example.test",
  "password": "replace-with-a-private-password"
}
```

For a global account, send `POST /api/v1/admin/login` with the same fields except
`tenant_id`, which must be omitted. Both return HTTP 200:

```json
{
  "contract_version": 1,
  "session": {
    "token": "<one-time-delivered-bearer-session>",
    "token_type": "Bearer",
    "expires_at_millis": 1800000900000,
    "account": {
      "id": "00000000-0000-0000-0000-000000000002",
      "username": "operator@example.test",
      "authority": {"kind": "tenant", "tenant_id": "example-company"},
      "credential_id": "00000000-0000-0000-0000-000000000001",
      "revision": 1,
      "disabled_at_millis": null,
      "history": [{
        "revision": 1,
        "action": "login_account_created",
        "actor_id": "00000000-0000-0000-0000-000000000001",
        "at_millis": 1800000000000
      }]
    }
  }
}
```

The token above is a placeholder, not a valid credential. Actual tenant session
tokens start with `qdbst1_`; global sessions start with `qdbsg1_`. Send the returned
token in `Authorization: Bearer <token>`. Never include it in a URL or log.
All responses carry `Cache-Control: no-store`. The API does not set cookies.

Sessions expire after at most 15 minutes, capped by the bound credential's expiry.
The server reads the current account, session verifier and underlying credential
on every authentication. `/identity` reports the underlying credential; the
session's separate expiry is the value returned by login. A session cannot extend
that deadline. Sign in again for a new session.

Each account retains at most 32 active session verifiers. Login removes expired
or obsolete sessions and evicts the oldest retained session if the limit is full.
Only verifiers are stored; tokens contain 256 random bits. Sessions survive a
server restart until their normal expiry or explicit invalidation.

## Password changes, disablement and logout

| Operation | Company route | Global route |
| --- | --- | --- |
| Inspect account | `GET /api/v1/login-accounts/{id}` | `GET /api/v1/admin/login-accounts/{id}` |
| Replace password | `POST /api/v1/login-accounts/{id}/password` | `POST /api/v1/admin/login-accounts/{id}/password` |
| Disable account | `POST /api/v1/login-accounts/{id}/disable` | `POST /api/v1/admin/login-accounts/{id}/disable` |

Password replacement accepts `contract_version`, `expected_revision` and
`password`. Disablement accepts `contract_version` and `expected_revision`.
Both require the same administrative authority as account creation and increment
an auditable revision. A stale revision returns 409. There is no anonymous reset
or account reactivation operation.

Changing a password or disabling an account atomically invalidates all its
sessions. Rotating the bound API key invalidates existing sessions; the account
can then sign in using the unchanged password and the current credential revision.
Revoking or expiring the bound credential also prevents new login. Disabling a
human account does **not** revoke separately issued API keys. Revoke those keys
explicitly when ending their authority.

`POST /api/v1/logout`, authenticated with a session, invalidates only that session
and returns `{"contract_version":1,"logged_out":true}`. A permanent API key is
not a logout token. Expired or already logged-out sessions return 401.
Administrative writes made through a session also compare its current account and
parent credential in the storage transaction. Already-authorized data operations
may finish during revocation; revocation does not cancel an in-flight request.

## Limits and failure handling

Unknown usernames, incorrect passwords, disabled accounts, accounts in cooldown
and revoked underlying credentials return the same 401 `invalid_login` response.
Unknown usernames incur Argon2 work without creating durable records. This follows
[OWASP authentication guidance](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html)
on generic errors and login throttling; it is not a claim of constant network timing.

- Five failed attempts cause a durable five-minute account cooldown. Attempts
  during the cooldown do not extend it. An authorized password replacement clears
  the cooldown; restarting the server does not.
- Per process, each username/authority pair gets at most eight attempts per minute,
  with 120 attempts per minute across all names. This admission limit returns 429
  `login_rate_limited`; wait at least one minute. Admission counters reset on
  process restart; the account cooldown remains durable.
- At most two password operations run concurrently, including account creation and
  password changes. Occupied capacity returns 503 `login_busy`; retry with backoff.
  This bounds password-hashing work independently of retrieval admission.
- Login and account routes accept at most 8 KiB of JSON. Invalid fields return 400,
  oversized bodies 413, missing credentials 401, denied authority 403 and storage
  or concurrent revision conflicts 409. Never overwrite newer account metadata
  merely to retry a conflict; inspect the current revision first.

Account-specific cooldowns can temporarily block a legitimate user targeted by
repeated failures. Keep the separately secured operator API key for administrative
recovery. Internet-facing deployments should also enforce appropriate edge abuse
controls. This native password contract does not provide MFA or federation.

Use the [administrative directories](administration-directory.md) to list authorized
accounts and credentials with bounded pagination.
