# Run QilbeeDB locally with Docker

Run a standalone QilbeeDB service to test application integration. The server
container stores durable platform data in a named volume and exposes HTTP on the
local machine. It does not generate embeddings or execute learned tool code.

## Build and provision

From the QilbeeDB repository:

```bash
QILBEE_REVISION="$(git rev-parse HEAD)" docker compose build
```

For a new, unused data volume, provision its first operator before starting the
server. The command needs exclusive access to that volume:

```bash
umask 077
mkdir -p "$HOME/.config/qilbeedb/local"
chmod 700 "$HOME/.config/qilbeedb/local"
(set -C; docker compose run --rm --no-deps qilbeedb \
  bootstrap-tenant /data example-company platform-operator \
  > "$HOME/.config/qilbeedb/local/admin.json")
chmod 600 "$HOME/.config/qilbeedb/local/admin.json"
```

The file contains the one-time operator secret. Keep it outside source control.
`set -C` prevents overwriting an existing credential file. Bootstrap does not
reset a tenant. For an existing installation, keep its saved credentials and data
volume; routine issuance and rotation use the authenticated HTTP API.

## Start and inspect the service

```bash
docker compose up -d
docker compose ps
curl --fail http://localhost:7474/health
```

Open the [API reference](http://localhost:7474/docs) in your browser. The exact
OpenAPI contract is available at [openapi.json](http://localhost:7474/openapi.json).
`localhost` refers to this machine. Docker service names resolve between
containers on a shared network and are not host-browser DNS names.

The default Compose configuration binds to `127.0.0.1:7474`, runs as UID/GID
10001, mounts the data volume at `/data`, uses a read-only root filesystem and
an ephemeral `/tmp`, drops Linux capabilities and disables privilege escalation.
A process health check does not verify your credential's scope or a complete
application workflow; perform authenticated memory writes and reads as well.

## Preserve data across updates

Use an explicit image tag and record its Git revision for each validated build.
Recreate only the QilbeeDB service with the same data volume and credentials.
Removing the volume removes durable state; it is not a routine upgrade step.
Check the running version and rerun your integration tests after deployment.
A process-restart test demonstrates that deployment's recovery behavior; it does
not by itself prove host power-loss or storage-hardware guarantees.

See the [operator Docker reference](../operations/docker.md) for image details
and optional network configuration. Network-facing deployments need appropriate
TLS termination and operator-managed credentials.

## Retrieval capacity

See [configure retrieval capacity](../operations/retrieval-capacity.md) for support
through 32,768 dimensions, including 3,072, explicit scan-byte budgets and bounded
concurrent retrieval. The ranking catalog reports the active operator settings.
