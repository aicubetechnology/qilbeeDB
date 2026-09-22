# Knowledge about externally owned tools

## Responsibility boundary

Tools belong to the application and the company whose tasks they serve. The
application owns their code, packages, dependencies, maintenance, release
selection, business permissions and operational lifecycle. Its infrastructure
runs tools with appropriate isolation. A lightweight agent client need not host
a compiler, container runtime or execution server.

QilbeeDB stores knowledge about tools: purpose, applicability, instructions,
preconditions, limitations, examples, observed outcomes and provenance. External
version references associate knowledge with the implementation that produced an
observation. They do not transfer ownership of that implementation to the bank.

| Responsibility | Owner |
| --- | --- |
| Create, repair, package and publish executable tools | Agent application and company |
| Select a tool for a business task and authorize its execution | Agent application under company policy |
| Dispatch, cancel, recover and account for execution | Application execution infrastructure |
| Persist and retrieve scoped knowledge and its source history | QilbeeDB |
| Evaluate recorded knowledge under an explicit evidence policy | QilbeeDB, with observations and evaluation context supplied externally |
| Determine whether an external version is available and permitted now | Agent application and execution infrastructure |

Persisting evidence does not make the database the owner of the business process.
A knowledge qualification decision is not permission to publish or execute code.

## Existing capabilities and compatibility

The [Learned Tool API](../api/learned-tools.md) currently exposes source-text
artifacts, executor profiles and development outcome records. These existing
interfaces remain documented for compatibility. They are not prerequisites for
ordinary memory or procedural knowledge, and their existence does not establish
the intended ownership of future capabilities. They do not dispatch programs or
publish executable releases.

The previous architecture that assigned development, packaging and an execution
gateway to QilbeeDB is superseded. Any migration or removal of existing interfaces
requires usage assessment, an explicit compatibility plan and agreement with
affected consumers. This document does not remove endpoints or migrate records.

The [Procedural Learning API](../api/procedural-learning.md) already supports
instructions, source references and immutable evaluation contexts. Contexts can
identify external tool revisions without storing executable code. An exact
revision string is a declared identity, not independent proof of an external
artifact's existence, integrity or current availability. A schema or catalog
revision identifies an offered interface; it must not be presented as an
executable implementation revision.

## Evidence and current observations

Useful knowledge should identify its originating observations and applicable
context. Comparisons must distinguish the baseline, candidate instructions,
model, tools, dataset, harness, environment and permissions. A standalone tool
comparison and an entire agent-task comparison answer different questions; do
not silently substitute one identity or claim for the other.

A historical receipt records what was accepted at that time. A current read must
distinguish qualification state from the current validity of source memories.
Updated, rejected, deleted or expired evidence can invalidate reuse. Expiry may
occur without a change-feed event. Incomplete traversal, missing evidence and
unknown resource consumption must remain explicit, never counted as success.

The additional generic contract binding procedural knowledge to exact memory
source revisions is under cross-team review. Its fields and endpoints are not
available contracts. The unpublished artifact-dependent candidate design is
suspended; no code registration or executor profile should be required by its
replacement.

Knowledge inspection does not reserve execution or eliminate the race between
inspection and use. Current business authorization and execution recovery remain
application responsibilities. The database must not fetch arbitrary external
locations or invoke code merely to inspect a knowledge reference.

## Ownership review before implementation

Before adding a capability, identify the user need, existing owner, alternatives,
data and decision authority, security boundaries and lifecycle responsibilities.
Database persistence alone is not sufficient justification for database ownership.

When the change affects a shared boundary, both teams must explicitly agree on
ownership, the integration contract, acceptance evidence and compatibility impact
before implementing the dependent change. Silence is not agreement. Independent
investigation and internal work may continue. Reopen the review when scope changes.

Validate source integrity, tenant and private-subject isolation, current state,
recovery and bounded work independently of retrieval quality. Claims of better
tool selection or agent capability additionally require comparative task evidence.
