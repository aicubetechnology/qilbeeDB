# Docker

Run the standalone QilbeeDB platform in the local Docker engine for integration
testing. The repository supplies a multi-stage `Dockerfile`, `compose.yaml`, and
an optional `compose.qmn.yaml` network attachment. No QMN services are required by
the default deployment.

## Build from source

```bash
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeeDB
QILBEE_REVISION="$(git rev-parse HEAD)" docker compose build
```

The image uses digest-pinned Rust 1.93.1 and Debian Bookworm bases, the committed
Cargo lockfile, and a release build with thin LTO. Build concurrency is two jobs
to fit local development machines. Dependency/target caches belong to BuildKit;
the build context excludes local data, credentials, Git state and host targets.
The runtime includes the server, required native libraries, CA certificates,
curl for health checks, and the license. It runs as UID/GID 10001.

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
  bootstrap-tenant /data qilbee-qmn-local platform-operator \
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

## Optional Qilbee/QMN network

The local QMN installation uses an existing Docker network. Attach QilbeeDB as
an additional service without recreating QMN containers:

```bash
QMN_DOCKER_NETWORK=qilbee-mycelial-network_qmn-network \
  docker compose -f compose.yaml -f compose.qmn.yaml up -d
```

Containers attached to that network can use this internal Docker DNS name
(the host browser cannot resolve it):

```text
http://qilbeedb-local:7474
```

In the browser on the Docker host, use `http://localhost:7474/health` or
`http://localhost:7474/openapi.json`. A `DNS_PROBE_FINISHED_NXDOMAIN` error for
`qilbeedb-local` in the host browser means the internal container hostname was
used outside its Docker network.

Network clients still need QilbeeDB platform credentials and explicit resource grants.
Attaching a network does not migrate QMN data, change QMN's decision authority,
or configure its applications automatically. Other applications can use the same
API. The standalone Compose file has no dependency on this external network.

## Configure the Compose deployment

| Variable | Default | Meaning |
| --- | --- | --- |
| `QILBEE_IMAGE` | `qilbeedb:local` | Local image tag to build/run |
| `QILBEE_REVISION` | `unknown` | OCI image revision label during build |
| `QILBEE_HTTP_PORT` | `7474` | Host loopback port mapped to container port 7474 |
| `RUST_LOG` | `info` | Server tracing filter |
| `QMN_DOCKER_NETWORK` | `qilbee-mycelial-network_qmn-network` | Optional external network used only with the QMN override |

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
retrieval, procedural learning and learned-tool development receipts. Generation
workers, isolated invocation transport and tool publication gates remain
subsequent features under the [accepted architecture](../architecture/learned-tools.md).
This development deployment does not establish distributed availability,
hardware power-loss behavior or comparative performance leadership.

## Recorded local acceptance

The 0.2.0 platform increment was validated on Docker Desktop Linux ARM64 with
the following results:

- Standalone startup and scoped API operation without the QMN network attached.
- Authentication, restricted capabilities, tenant/private-subject isolation,
  create/read/update/query/delete, stale revisions and idempotency conflicts.
- Five acknowledged records and their original receipts survived `SIGKILL` and
  container restart on the persistent volume. Retrying a deleted create did
  not resurrect its record.
- Live response bodies validated against the served OpenAPI 3.1 schemas.
- The optional QMN network attachment served the health endpoint to a separate
  client container. Existing QMN containers were not reconfigured.
- The full Rust workspace passed 355 tests; two subprocess fixtures are marked
  ignored for direct discovery and invoked by their parent recovery tests.

The local smoke suite does not establish power-loss durability, distributed
availability, execution isolation for future learned tools, or compatibility
with unmodified QMN applications. The PR records the tested revision.

## Procedural release acceptance (0.3.0)

The expanded release passed 373 Rust workspace tests and four local Docker
acceptance tests. The procedural Docker cycle validated all request/response
bodies against the served schemas, administrative denial, eight simultaneous
retries counting as one case, 32 accepted paired cases, explicit unknown
accounting, model-version fallback and private-subject denial. After SIGKILL,
the active procedure, original proposal and all 32 evaluation receipts were
recovered. Subsequent negative monitoring suspended the procedure and selected
the exact baseline. Temporary evaluator credentials were revoked.

The browser reference at `/docs` loaded version 0.3.0 with 22 operations; endpoint
filtering and layout were checked in a browser. The page loads its script and
OpenAPI document from the same server and does not collect credentials.

These are synthetic contract and process-recovery tests. They do not execute a
model, verify external evidence truth or demonstrate improvement on an agent
benchmark. The earlier memory/isolation/crash tests also passed on this image.

## Tools and semantic release acceptance (0.4.0)

The 0.4.0 release passed 403 Rust workspace tests and six Docker acceptance tests
on Linux ARM64. The candidate image was first tested in a separate container with
its own disposable volume and explicit loopback port. The same suites are used
to validate the merged image on the persistent local deployment.

The new suites validate worker/developer/admin separation, immutable artifacts,
unknown development consumption, eight concurrent identical reports producing
one event, atomic successful artifact registration, scoped repair requests and
executor-confirmed cancellation. A process kill preserves the original request,
unknown and successful events, and exact artifact digest. All temporary worker
and administrator credentials are revoked after validation.

Semantic acceptance supplies three synthetic vectors with expected cosine
scores 1, 0 and -1, validates response schemas, tenant/capability denial and
partial-scan disclosure, then kills/restarts the container and replays the
original embedding receipts. Source updates immediately exclude stale vectors;
deleting the source excludes all its bindings. The browser contract now exposes
32 operations and 94 schemas, including model identities and scan coverage.

These tests establish transport, ranking, authority and process-recovery
contracts. They do not measure semantic quality on a language dataset or verify
external generation/test evidence. Embeddings come from an external model
service. Artifact source is not executed by the database; configured development
workers, isolated invocation transport and publication gates remain separate
integration work. Exact bounded retrieval is not an ANN performance claim.

## Retrieval validation in 0.5.0

The 0.5.0 release preserves cosine search and adds explicit lexical and
experimental server-versioned hybrid endpoints. Its isolated Docker acceptance
covers authorization, scoped lifecycle, OpenAPI response schemas, process-crash
recovery and replay of a frozen 40-record retrieval fixture. The synthetic
comparison did not qualify hybrid relevance; see the
[report and limits](../research/retrieval-contract-report.md).

English Markdown user-guide sources are exported into the sibling
`qilbee-site/app/src/doc/qilbeedb` directory. The exporter and PR documentation
workflow are described in the repository's `CONTRIBUTING.md`. After a validated
release, the site export uses `--status released`; the hybrid ranking profile
continues to report `experimental: true`.
