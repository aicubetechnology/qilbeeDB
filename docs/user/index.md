# QilbeeDB documentation

Build agent applications with durable memory, scoped retrieval and traceable
learning records. QilbeeDB stores evidence and retrieves it under explicit
identity and revision contracts. Your application chooses the language model,
generates embeddings externally and decides how retrieved evidence is used.

## Start building

- [Quickstart](quickstart.md): create a memory and retrieve it with a scoped credential.
- [Authentication and scopes](../security/scoped-credentials.md): understand tenants, resource grants and private subjects.
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

## Build learning workflows

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

The platform rejects unknown request fields and applies a 65536-byte JSON body
limit. Start with the [HTTP contract and errors](../api/platform-http.md) when
integrating a new client. The current default router covers the documented
platform endpoints; legacy graph and memory routes are a separate compatibility
surface.
