<div align="center">

<img src="docs/assets/qilbeedb-logo.png" alt="QilbeeDB" width="560">

**Graph database and evidence-driven memory for AI agents**

[Documentation](docs/user/index.md) · [Quickstart](docs/user/quickstart.md) · [API reference](docs/api/openapi.json) · [Contributing](CONTRIBUTING.md)

Created by **[AICUBE TECHNOLOGY LLC](https://www.aicube.ca/)**

</div>

## What is QilbeeDB?

QilbeeDB is a Rust database for durable agent memory, explicit graph relations,
scoped retrieval and evidence-bound procedural knowledge. Applications can use
it through the QilbeeDB platform or run their own installation.

The database owns persistence, authorization, revisions, retrieval and evidence
validation. Your application owns agent and project identifiers, model selection,
embedding generation, business decisions and tool code, maintenance and execution.
Storing or retrieving knowledge does not itself demonstrate better agent reasoning.

**Current release: [0.14.0](docs/releases/0.14.0.md).** Check your server's `/health`
and `/openapi.json` before using a capability. The repository can contain source
previews beyond a deployed release; guides identify their availability. Hybrid
and graph-assisted ranking remain experimental and opt-in.

## Choose how to use QilbeeDB

| | QilbeeDB platform | Self-hosted |
| --- | --- | --- |
| Connect | `https://api.qilbeedb.io` | Your installation's base URL |
| Credentials | Obtain an application key from your company administrator | Provision the installation and issue application credentials |
| Administration | [Company console](https://admin.qilbeedb.io) and authorized APIs | Administration APIs and the console available for your installation |
| Operations | Service infrastructure is managed by the platform | Your organization manages storage, TLS, backups and updates |
| Start | [Create and retrieve a memory](docs/user/quickstart.md) | [Install with Docker](docs/user/deployment.md), then use the same quickstart |

Platform users do not need to install Docker or bootstrap a local database.

## Capabilities

| Area | Available behavior | Guide |
| --- | --- | --- |
| Durable memory | Idempotent writes, conditional revisions, provenance, review, expiry and deletion | [Memory API](docs/api/versioned-memory.md) |
| Authorization | Company identity, credential capabilities, authorized scopes, private subjects, rotation and revocation | [Scoped credentials](docs/security/scoped-credentials.md) |
| Agent registration | Application-owned IDs and authorized registration on first use | [Agent registration](docs/security/agent-registration.md) |
| Graph memory | Revision-bound typed relations, traversal and explicit retrieval evidence | [Graph-assisted retrieval](docs/api/graph-assisted-retrieval.md) |
| Context freshness | Durable changes, checkpoints and consumer diagnostics; expiry requires current eligibility checks | [Context consumers](docs/api/verified-memory-consumer.md) |
| Procedural knowledge | Versioned procedures, evaluation outcomes and applicability bound to current evidence | [Evidence-bound knowledge](docs/agent-memory/evidence-bound-knowledge.md) |
| Graph consolidation | Leased work and fenced publication of externally inferred relations | [Consolidation API](docs/api/graph-consolidation.md) |
| Administration | Company resource discovery, memory inspection, agent display names and learning history | [Administration](docs/security/administration-directory.md) |
| Native graph storage | Durable graph lifecycle, atomic entity/index commits and optimistic point-read conflict checks | [Transactions](docs/architecture/atomic-commits.md) |

### Retrieval with external embeddings

| Method | Request input | Score |
| --- | --- | --- |
| [Lexical](docs/api/lexical-memory.md) | Text | BM25 |
| [Semantic](docs/api/semantic-memory.md) | External vector and exact model identity | Cosine |
| [Hybrid](docs/api/hybrid-memory.md) | Text, vector and explicit ranking version | Server-versioned rank fusion |
| [Graph-assisted](docs/api/graph-assisted-retrieval.md) | Retrieval seeds and explicit graph profile | Base and eligible typed-path contributions |

Embeddings stay outside the database. Supply provider, model, immutable revision
and dimensions for both stored and query vectors. The platform supports vectors
through **32,768 dimensions, including 3,072**, subject to configured request and
work limits. Equal dimensions do not make different model spaces compatible.

Scopes are applied before candidate selection. Inspect response coverage and
work limits: a bounded result is not necessarily an exhaustive ranking. Combined
scores are neither cosine nor probabilities. Choose methods from measured results
on your workload; a newer profile is not automatically more relevant.

## Quickstart

Obtain an application credential with `memory_read` and `memory_write` for the
scope you intend to use. Administrative authority does not automatically grant
application memory permissions. Keep secrets outside source control.

```bash
# Platform connection; for self-hosting, use your installation's URL.
export QILBEE_BASE_URL="https://api.qilbeedb.io"
curl --fail --silent --show-error "${QILBEE_BASE_URL}/health"
```

Follow the [memory quickstart](docs/user/quickstart.md) to create a memory, retain
its receipt and retrieve it by text. That guide includes complete authenticated
requests, expected responses and safe update/retry behavior. Add external vectors
and explicit relations when your application needs them.

### Self-hosted installation

```bash
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeeDB
QILBEE_REVISION="$(git rev-parse HEAD)" docker compose build
```

Before starting a new installation, follow the [provisioning and startup guide](docs/user/deployment.md).
It uses a persistent data volume and protects the initial operator secret.
Use an explicitly validated image/revision for your deployment. Preserve the
volume and credentials during updates; never treat volume removal as an upgrade.
For 0.14.0, read the [property-index compatibility and rollback requirements](docs/architecture/property-index.md).

## Security and recovery

The current platform uses authenticated company and subject identities with
capabilities and resource scope policies. Global installation authority and
company administration are distinct. See [global administration](docs/security/global-administration.md),
[human login](docs/security/human-login.md) and [company integrations](docs/security/company-integrations.md).

For network-facing installations, configure TLS, credential lifecycle and backup
procedures. Follow the [container security](docs/security/container-security.md)
and [backup and recovery](docs/operations/backup.md) guides. Reconcile uncertain
writes using their original operation identity before retrying. Process-restart
validation does not establish host power-loss or storage-hardware guarantees.

Report suspected vulnerabilities privately to **contact@aicube.ca**.

## Evidence and limitations

Published evaluations include [external embeddings](docs/research/external-embedding-evaluation.md),
[SciFact retrieval](docs/research/scifact-results.md) and
[graph retrieval](docs/research/graph-retrieval-results.md). Reports distinguish
functional contract checks, retrieval relevance, coverage, latency and agent-task
evidence. Dataset size, sparse judgments and experimental conditions limit what
can be concluded. No state-of-the-art leadership or automatic agent-improvement
claim follows from these results.

Cypher support is partial; Bolt is a placeholder, and Neo4j compatibility is not
established. Distributed clustering and gRPC are not current platform guarantees.
Native transaction conflict checks do not provide transaction-start snapshots or
predicate/range isolation. Legacy routes and SDK examples have separate contracts;
use the current [HTTP API](docs/api/platform-http.md) for new memory integrations.

Tool knowledge describes applicability and evidence. Tool implementation and
execution belong to the application; see the [tool ownership boundary](docs/architecture/learned-tools.md).
Existing artifact APIs do not make the database an execution runtime.

## Development and contribution

The workspace uses Rust edition 2024; see the repository configuration for build
dependencies. Core crates separate storage, graph operations, memory, query parsing,
protocols and the HTTP server.

```bash
cargo build --locked --release
cargo test --locked --workspace --lib
cargo run --locked -p qilbee-memory --example learning_cycle
```

The learning-cycle example demonstrates evaluation, promotion, reopening and
suspension using deterministic inputs. It is not an external agent benchmark.

Read [CONTRIBUTING.md](CONTRIBUTING.md) for validation, documentation and release
requirements. Preserve compatibility, validate recovery and isolation, and keep
claims proportional to the evidence. Report bugs or propose features through
[GitHub Issues](https://github.com/aicubetechnology/qilbeeDB/issues).

## License

See [LICENSE](LICENSE) for the governing terms and permitted uses. For commercial
licensing questions, contact **licensing@aicube.ca**.

[Website](https://qilbeedb.io/) · [User documentation](docs/user/index.md) · [API contract](docs/api/openapi.json)
