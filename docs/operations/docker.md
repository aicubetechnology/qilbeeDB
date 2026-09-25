# Docker

Run the standalone QilbeeDB platform in the local Docker engine for integration
testing. The repository supplies a multi-stage `Dockerfile`, `compose.yaml`, and
optional network configuration. No additional application services are required by
the default deployment.

## Build from source

```bash
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeeDB
QILBEE_REVISION="$(git rev-parse HEAD)" docker compose build
```

The image uses a digest-pinned Rust 1.93.1 Bookworm build stage and a
Distroless Debian 13 C/C++ runtime, the committed Cargo lockfile, and a release
build with thin LTO. Build concurrency is two jobs. Dependency/target caches
belong to BuildKit; the build context excludes local data, credentials, Git state
and host targets.

The runtime includes the server, required native libraries, CA certificates,
a built-in loopback health probe, and the license. It runs as UID/GID 10001,
with `/data` owned by that user and initially restricted to mode 0700. Existing
volumes retain their own permissions; verify access before starting the service.

The service image does not include a shell, package manager or general-purpose
backup utilities. Invoke native commands directly, for example
`docker compose exec qilbeedb /usr/local/bin/qilbeedb health-check`.
Use your host or separately maintained operator tools for volume preparation,
backup transport and diagnostics. Scripts that depend on `docker exec ... sh`,
`tar`, or package installation inside the service need updating before adoption.
See [container security](../security/container-security.md) and
[backup and recovery](backup.md).

The resulting default image is `qilbeedb:local`. This builds locally; it does not
publish an image to a registry. Use an explicit image tag and Git revision when
recording a validated deployment.

## One-time local provisioning

Provision before starting the service, using the same named volume:

```bash
umask 077
mkdir -p "$HOME/.config/qilbeedb/local"
chmod 700 "$HOME/.config/qilbeedb/local"
(set -C; docker compose run --rm --no-deps qilbeedb \
  bootstrap-tenant /data example-company platform-operator \
  > "$HOME/.config/qilbeedb/local/admin.json")
chmod 600 "$HOME/.config/qilbeedb/local/admin.json"
```

The JSON file contains the one-time operator secret. Keep it outside Git and do
not paste it into logs or documentation. Bootstrap creates the tenant once; it
will not reset an existing tenant. The subshell uses `set -C` to refuse
overwriting an existing credential file. Do not rerun the command over an existing
credential file, because shell redirection would truncate that file before the
command fails. For an existing installation, use its saved credential and volume.

The local operator command needs exclusive access to the volume's RocksDB files.
Stop the service before using that command. Routine key issuance, rotation and
revocation use the authenticated HTTP API while the service is running.

## Start the standalone service

```bash
docker compose up -d
docker compose ps
curl --fail http://localhost:7474/health
curl --fail http://localhost:7474/openapi.json
# Browser reference: http://localhost:7474/docs
```

Compose publishes HTTP on **127.0.0.1:7474** by default. The root filesystem is
read-only, `/data` uses the persistent named volume, `/tmp` is temporary, Linux
capabilities are dropped, and privilege escalation is disabled. The server has
no learned-code executor in this container.

The health check reports process availability. Successful scoped writes and
reads are separate integration checks. Only HTTP is exposed; this deployment
does not advertise an implemented Bolt listener.

## Connect application containers

Use a Docker network shared by the application and database containers. Docker
service names resolve inside that network; a browser on the host should use the
published host address, such as `http://localhost:7474`.

Network access does not grant API permissions. Applications still need an API key
with the appropriate capabilities and scope. Configure the network according to
your application's deployment, without recreating unrelated services.

## Configure the Compose deployment

| Variable | Default | Meaning |
| --- | --- | --- |
| `QILBEE_IMAGE` | `qilbeedb:local` | Local image tag to build/run |
| `QILBEE_REVISION` | `unknown` | OCI image revision label during build |
| `QILBEE_HTTP_PORT` | `7474` | Host loopback port mapped to container port 7474 |
| `RUST_LOG` | `info` | Server tracing filter |

These are actual Compose controls. The server does not currently read the older
illustrative `QILBEE_DATA_PATH`, `QILBEE_BOLT_PORT` or mounted `config.toml` examples.
Inside this image, the command explicitly selects `/data`.

## Validate the API

1. Read `/health` and import `/openapi.json` into your API client.
2. Use the operator key to issue a restricted integration credential with
   `memory_read` and `memory_write`, plus exact project/mission/agent grants.
3. Create a memory with a unique idempotency key; retry it and compare receipts.
4. Read, conditionally update, query and delete through the
   [versioned memory contract](../api/versioned-memory.md).
5. Verify that another tenant and a read-only credential cannot mutate it.
6. Restart the container and verify a retained test record and its original
   receipt. Remove the test record through an authorized delete command.

The [HTTP reference](../api/http-api.md) and [platform administration
reference](../api/platform-http.md) contain authentication, payloads, response
shapes, error codes and migration details. Existing legacy SDKs require an adapter
before they can use the versioned platform routes.

## Persistence, update and rollback

```bash
# Restart the same instance; the named volume is retained.
docker compose restart

# Inspect the image revision actually serving requests.
docker inspect qilbeedb-local \
  --format '{{ index .Config.Labels "org.opencontainers.image.revision" }}'

# Stop the service while retaining its data.
docker compose down
```

Do not use `down --volumes` or delete the named volume when preserving memory and
credential state. This release does not include a validated backup/restore tool.
Take an appropriate offline volume backup before testing storage-format changes.

To update, build a new explicit image tag and recreate only the QilbeeDB service
with that tag. Keep the prior image for rollback. A prior binary must understand
the stored schema versions before it can safely serve the volume; unsupported
versions fail closed. Do not prune unrelated images, containers or volumes.

## Scope of the local deployment

The local stack supports durable identity, scoped memory, model-bound semantic
retrieval, provenance and observed experience. Tool implementation and execution
remain with the consuming application and its execution infrastructure.
This development deployment does not establish distributed availability,
hardware power-loss behavior or comparative performance leadership.
