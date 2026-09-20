# Immutable tool artifacts

`LearningMemory::register_tool_artifact` stores source revisions in the durable
learning database. It performs no imports, builds, model calls or program
execution. The trusted Rust interface requires its caller to authorize tenant,
scope and actor; HTTP authority is documented with the platform API.

A `ToolArtifactProposal` contains an ID, UTF-8 source, dependency lock text,
content-addressed runtime image (`sha256:` plus 64 lowercase hexadecimal digits),
entrypoint, input/output schema declarations, source references and optional
repair parent/evidence. It receives a `ToolArtifact` with the authenticated actor,
timestamp, tenant, namespace and three digests:

- `source_digest`: SHA-256 of the exact UTF-8 source bytes.
- `dependency_digest`: SHA-256 of the exact dependency lock bytes, including an
  empty lock when no dependencies are declared.
- `artifact_digest`: SHA-256 of the Rust serializer's JSON representation of the
  complete typed proposal, including its identity and provenance. Clients should
  preserve the returned digest instead of hashing differently formatted JSON.

An ID identifies one immutable revision within a tenant and namespace. An
identical proposal returns the original record and actor. Changed content under
that ID conflicts, including changes to dependencies, schemas or provenance.
Repairs require both an existing parent in the same namespace and a nonempty
failure-evidence reference. New records cannot point to themselves; immutable
parents must already exist, so successful writes cannot create lineage cycles.
Different tenants and private subject namespaces cannot resolve each other's
parents. Registration synchronizes the WAL before returning, and readers verify
stored identity and content digests. Concurrent identical registration returns
one original record.

## Limits and interpretation

IDs and entrypoints support 1–512 UTF-8 bytes. Source supports 1–24576 bytes;
dependency locks support 0–8192 bytes. Each schema must be a JSON object or boolean
whose serialized representation is at most 8192 bytes. Source references number
1–32, each 1–2048 bytes. A repair evidence reference supports 1–2048 bytes. HTTP
requests also share the platform's total transport limit.

Schemas, dependencies, image digests and provenance are declarations. This store
does not validate the complete JSON Schema language, fetch dependencies or
images, verify externally referenced evidence, or attest to runtime isolation.
Store no credentials in source or lock files. Registration does not authorize
execution, prove test success, or publish a tool. Development, evaluation,
publication and execution need their own authenticated contracts.

## External executor profiles

`register_tool_executor(tenant, profile, actor)` registers an immutable
`ToolExecutorProfile`. It binds an executor ID to an authenticated subject, exact
runtime image digest, environment revision, permissions revision and positive
`max_cost_units` / `max_latency_ms` limits. Identities support 1–512 UTF-8 bytes;
budgets are unsigned 64-bit integers. The returned `ToolExecutor` preserves the
profile digest, registering actor and timestamp. `tool_executor` verifies its
identity and digest on read. Identical retries return the original registration;
changed profiles require a new ID. Profiles are tenant-wide administrative
contracts, not caller-supplied scope grants.

Profiles contain no passwords, endpoint URLs or executable transport settings.
Configured server workers contact the platform with separate scoped credentials.
The registered subject must match the authenticated reporting subject; possession
of a profile ID is insufficient. Credential rotation preserves subject identity,
while revocation or expiry blocks later HTTP reports. A profile alone does not
start workers, grant network access, enforce an operating-system sandbox or
attest that its declared image was executed. Those properties need independent
executor deployment and verification. Resource values are reported evidence in a
shared deployment-defined accounting unit, not a billing integration.
