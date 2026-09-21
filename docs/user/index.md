# QilbeeDB documentation

Build agent applications with durable memory, scoped retrieval and traceable
learning records. QilbeeDB stores evidence and retrieves it under explicit
identity and revision contracts. Your application chooses the language model,
generates embeddings externally and decides how retrieved evidence is used.

## Start building

- [Quickstart](quickstart.md): create a memory and retrieve it with a scoped credential.
- [Authentication and scopes](../security/scoped-credentials.md): understand tenants, resource grants and private subjects.
- [Automatic agent registration, 0.12.0](../security/agent-registration.md): register external IDs on successful authorized requests and inspect the company directory.
- [Company integrations, 0.12.0](../security/company-integrations.md): authorize external project and agent IDs through a versioned policy and change access without replacing keys.
- [Company memory administration, 0.12.0](../security/company-memory-administration.md): discover retained workspaces and inspect company records independently of credential grants.
- [Memory API](../api/versioned-memory.md): create, update and delete records with durable receipts and revision checks.
- [Local Docker deployment](deployment.md): run the platform and locate its API reference.

## Choose a retrieval mode

| Mode | Input | Ranking signal | Use when |
| --- | --- | --- | --- |
| [Lexical](../api/lexical-memory.md) | Text | BM25 | Exact words and identifiers matter, or no embedding is available |
| [Semantic](../api/semantic-memory.md) | External vector and model identity | Cosine | Meaning should be matched through your selected embedding model |
| [Hybrid, experimental](../api/hybrid-memory.md) | Text, external vector and ranking version | Weighted reciprocal rank fusion | You want to evaluate complementary lexical and semantic candidates |

Each mode isolates the authorized tenant, project, agent, mission and private
subject before candidate selection. Responses distinguish ranking scores from
probabilities and disclose bounded scan coverage. The hybrid endpoint uses an
immutable server-owned profile; it does not change the cosine score returned by
the semantic endpoint.

[Evaluate retrieval](../research/retrieval-evaluation.md) on frozen, judged queries
before adopting a ranking version. Report retrieval quality separately from
end-to-end agent task outcomes.

## Build learning workflows

[Experience receipts](../api/experiences.md), introduced in the 0.7.0
contract, preserve execution intent, authenticated observations and unknown resource
consumption. They do not automatically qualify a procedure.

[Procedural learning](../api/procedural-learning.md) records proposals, evaluation
evidence and publication decisions. [Learned tools](../api/learned-tools.md) add
immutable artifacts, executor profiles and a durable development ledger. Workers
execute tool development outside the database process. These contracts make
outcomes observable; they do not guarantee that every learned procedure improves
an agent or that a generated program is safe merely because it was recorded.

## Use the exact deployed contract

The server publishes its OpenAPI document at `/openapi.json` and an interactive
reference at `/docs`. On the local Docker host, open
[http://localhost:7474/docs](http://localhost:7474/docs). Check `/health` for the
running version. Release-stage metadata in these Markdown sources distinguishes
preview documentation from a validated deployment.

The platform rejects unknown request fields. Vector attachment, cosine search and
hybrid search accept JSON bodies up to 2 MiB; other routes retain the 65536-byte
limit. External vector spaces support 1–32768 dimensions, including 3072, subject
to the [operator capacity configuration](../operations/retrieval-capacity.md). Start with the [HTTP contract and errors](../api/platform-http.md) when
integrating a new client. The current default router covers the documented
platform endpoints; legacy graph and memory routes are a separate compatibility
surface.

[Evaluate experience evidence](../research/experience-evaluation.md) explains how
to freeze observations, preserve incomplete outcomes and compare agent tasks.

Use the [memory change feed](../api/memory-changes.md) to reconcile scoped caches
and resume from a durable cursor after disconnection.

Use [memory review](../api/memory-review.md) to record decisions and exclude
rejected revisions while preserving historical evidence.

Use [derived memories](../api/derived-memory.md) to keep conclusions dependent on
exact source revisions and transitive source eligibility, with bounded diagnostics.

Use [consumer checkpoints](../api/memory-checkpoints.md) to persist each subject’s
feed progress and reconnect safely after a process restart.

For embedded Rust graphs, see the 0.12.0
[durable graph lifecycle](../architecture/durable-graph-lifecycle.md) contract.
The [graph memory evidence map](../research/graph-memory-evidence.md) explains
which research informs the next memory integration and what remains unimplemented.

Use the 0.12.0 [memory evidence graph API](../api/memory-evidence-graph.md) to
read revision-bound ancestry in a single authorized snapshot, including native
company administration and explicit traversal coverage.
