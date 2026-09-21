# Bootstrap installation and tenant authority

QilbeeDB's default platform router has no built-in password, anonymous tenant
registration or implicit administrator account. Starting the server does not
grant authority. Use an explicit local operator command with exclusive access
to the persistent database directory.

## Multi-company installations

For a SaaS deployment or an installation that manages multiple companies,
bootstrap the installation master once:

```sh
umask 077
(set -C; qilbeedb bootstrap-master /var/lib/qilbeedb/data master \
  > /secure/operator/master.json)
```

The parent output directory must already exist with restricted permissions. The
command returns a one-time global credential. Move it to a secret manager and
use it to delegate narrowly scoped administrative service keys. A public signup
backend should receive only `tenant_create`; a browser receives no global key.

See [Global administration and SaaS provisioning](global-administration.md) for
the full authorization model, API, rotation, revocation and offline recovery.
The master is independent of tenant administrators and cannot be created or
reset through an HTTP endpoint.

## Explicit local tenant provisioning

An operator can also bootstrap an individual tenant while the server is stopped:

```sh
umask 077
(set -C; qilbeedb bootstrap-tenant /var/lib/qilbeedb/data example-company company-operator \
  > /secure/operator/company.json)
```

The new tenant credential has `credential_admin` and `policy_admin`, with no
implicit memory grants. It cannot register other companies or create global
credentials. Bootstrap rejects an existing tenant instead of resetting its
authority. For a running SaaS installation, prefer the authenticated registration
API and its durable provenance record.

Never run two processes against the same database directory. The local operator
commands use the same persistent storage as the server and require exclusive
access. Keep their standard output out of build logs, shell transcripts and Git.

## Start and verify

Start the server using the same directory and retrieve public `/health` and
`/openapi.json`. Then verify the issued key against the appropriate identity
endpoint:

- Global credential: `GET /api/v1/admin/identity`.
- Tenant credential: `GET /api/v1/identity`.

The endpoints are deliberately separate. Global keys do not become tenant data
credentials, and tenant keys never acquire installation authority. Use HTTPS for
network access. No login password or shared JWT signing secret is needed by the
default platform router.

The older interactive username/password bootstrap belongs to the explicitly
enabled legacy router. Its environment variables and bootstrap file are not the
security contract of the default platform API.
