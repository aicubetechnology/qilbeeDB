# Administrative directories

Administrative clients can enumerate companies, credentials and provisioned login
accounts through bounded, authorized directories. These endpoints return current,
sanitized metadata. They never return passwords, password hashes, API secrets or
session verifiers.

| Endpoint | Required authority |
| --- | --- |
| `GET /api/v1/admin/tenants` | Global `tenant_inspect` |
| `GET /api/v1/admin/credentials` | Installation master |
| `GET /api/v1/admin/login-accounts` | Installation master |
| `GET /api/v1/credentials` | `credential_admin` in the authenticated company |
| `GET /api/v1/login-accounts` | `credential_admin` in the authenticated company |

A human session can use the same endpoint when its underlying credential has the
required authority. A delegated global service credential cannot enumerate global
credentials or human accounts, even if it holds every global capability. Company
identity is derived from authentication; no query parameter can select another
company's credential or account directory.

## Request and response

Send `contract_version=1`, an optional `limit` from 1 to 100 (default 25), and the
previous page's opaque `cursor`, if present:

```http
GET /api/v1/credentials?contract_version=1&limit=25
Authorization: Bearer <company-administrator-session>
```

HTTP 200 returns `contract_version` and `page`, with an `items` array and a nullable
`next_cursor`. Credential and account items have the same sanitized shapes as their
individual inspection endpoints, including administrative revision history.
Company items contain `tenant_id` and `registration`; legacy companies created
through local bootstrap have a null registration, not a fabricated receipt.

Company registration also accepts an optional `display_name` containing 1–256
UTF-8 bytes without control characters. The immutable `tenant_id` remains the
security boundary; a display name never changes authorization. Existing clients
can omit the field. The display name is returned in the registration receipt.

## Pagination and coverage

The server selects the authorized directory prefix **before** seeking keys and
applying the page limit. Another company's records cannot consume a tenant's page
budget. Index keys use unambiguous JSON-encoded tenant identities.

A cursor is an exclusive position after the last returned logical key. It does not
contain authorization and is rejected when it falls outside the current directory.
Continue using the same endpoint and authority until `next_cursor` is null. Keys
advance without repetitions during an unchanged traversal.

Pages form a live traversal, **not a snapshot**. A concurrent insertion before the
cursor requires a fresh traversal to discover; record metadata can change between
pages. There is no exhaustive total count or claim that several pages represent
one point in time. Revoked credentials and disabled accounts remain visible for
administrative review. Their presence does not make them usable.

Each page contains at most 100 records and at most 4 MiB of serialized page data.
Reduce `limit` after a size-limit 400. Invalid page fields or foreign cursors return
400, invalid authentication 401 and insufficient authority 403. Stored index
inconsistency fails the whole request; the server does not silently omit a row.

## Existing data and startup

The engine maintains an ordered logical-key directory atomically with metadata
writes. The original metadata format has a length prefix, so treating its physical
keys as ordinary lexical prefixes would lose records of different key lengths.
The ordered directory is a separate internal index; credential values and their
histories keep the existing format.

Before serving requests, startup rebuilds these key markers from existing metadata
in batches of at most 1,000 keys, then repairs company-specific credential indexes
in pages of at most 100 records. The rebuild repeats on startup so records written
by an older binary are discovered. This is startup work proportional to metadata
size, not a background operation or constant-time availability promise. Existing
credentials, secrets, revisions and history are not rewritten by the repair.

## Browser administration origin

A deployment may set `QILBEE_ADMIN_ORIGIN` to one exact HTTPS origin, for example
`https://admin.example.test`. Paths, query strings, credentials and wildcards are
rejected at startup. When unset, the API does not enable cross-origin browser access.

The configured origin may send GET and POST with `Authorization` and
`Content-Type`; preflight results may be cached for 300 seconds. Cookie credentials
are not enabled. CORS is a browser transport policy, not database authorization:
every protected API request still needs its current bearer authority. Nonbrowser
clients continue to use the existing authentication contract.

A console should keep login sessions in memory, clear them on logout or expiry,
show one-time API keys only after explicit issuance or rotation, and handle a lost
write response as potentially committed. Inspect current metadata before retrying
an administrative write with a stale revision.
