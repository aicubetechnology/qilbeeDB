# Repository collaboration requirements

Use English for code, comments, documentation, commits and PR descriptions.
For every created and validated PR, update the relevant user guides in
`docs/user-docs.json` and export them to
`/Users/kimera/projects/qilbee-site/app/src/doc/qilbeedb` when that site workspace
is available. Run the export's `--check` mode and include validation in the PR.
See `CONTRIBUTING.md` for the reproducible export and release-stage workflow.

Preserve the user's uncommitted work. Never commit ignored tests, todo checklists,
Bruno files, generated MkDocs sites, credential files or local build artifacts.
Use externally supplied embeddings for platform retrieval. Preserve the existing
cosine endpoint's score semantics. Ranking combination parameters belong to
immutable server-owned versions, not arbitrary request weights. Hybrid relevance
and downstream agent improvement require separate empirical evidence.

## Permanent human-centered interface directive

Every QilbeeDB interface primarily serves a person completing a task. Apply this
requirement to new interfaces and changes to existing ones, including the private
corporate console, administration, onboarding, and error or recovery screens.

- Start with the person's goal and complete workflow, not the API's endpoint or
  identifier structure. Offer authorized discovery and selection for routine
  tasks; keep raw identifiers and contract editors in advanced workflows.
- Provide clear navigation, understandable language, consistent actions and a
  visual hierarchy that makes the next action and the current context obvious.
  Preserve the official QilbeeDB identity without sacrificing readability.
- Use progressive disclosure for infrequent controls and technical details. Keep
  essential results, active filters, access context, and material limitations
  visible. Never imply an empty filtered page is an empty company.
- Design accessibility into each interaction: semantic controls, visible labels,
  keyboard operation, visible focus, predictable focus management, sufficient
  contrast, responsive reflow, readable zoom, and non-color status cues. Provide
  a usable text/list alternative for graphs and other visual interactions.
- Give immediate, accessible feedback for pending actions, confirmed outcomes,
  errors, and recovery. Preserve safe input and confirmed progress. Distinguish
  uncertain outcomes from failure; do not repeat potentially committed writes
  under a new identity. Do not display stale or unauthorized data as current.
- Validate the complete user journey before publication, including discovery,
  selection, action, feedback, errors, and recovery. Check realistic permissions,
  empty and partial results, loading, slow or lost responses, retries, conflicts,
  session expiry, keyboard use, desktop, mobile, and zoom where applicable.
  Automated tests alone do not replace reviewing actual reading and interaction.
- Treat unresolved usability or accessibility failures in the changed journey as
  incomplete delivery. Record the journey, observed results, and remaining
  limitations; do not claim formal accessibility conformance without its audit.

These are permanent project acceptance requirements, not optional visual polish.
Do not mark a human workflow complete merely because its API calls work. An API
catalog gap that forces routine manual IDs remains product work to implement.
See `docs/contributing/interface-design.md` for the implementation and validation
checklist. Keep corporate UI source, deployments, and credentials out of Git.

## Mandatory capability ownership review

Before implementing any new or expanded capability, critically assess its owner:
QilbeeDB, the consuming agent/application, the company business domain, or the
execution infrastructure. Record the user need, proposed owner, alternatives,
why the database needs the capability, data and decision authority, security and
lifecycle responsibilities, and the smallest necessary integration contract.
Do not infer database ownership merely because data can be persisted there or
because an adjacent service already implements the capability.

For every decision affecting both teams, communicate this assessment through
`/Users/kimera/projects/colab` and obtain explicit agreement from Qilbee and
QilbeeDB before implementing the affected contract. Record the agreed boundary,
objections resolved, acceptance criteria, and compatibility/migration impact.
Silence is not approval. Read-only investigation and independent work may
continue; implementation dependent on an unresolved boundary must wait.
Reassess ownership when scope changes, including previously started work.

Tool code, packages, maintenance, publication and execution belong to the agent
application/company and its execution infrastructure. QilbeeDB may store scoped
knowledge about tools, external version references, provenance and observed
evidence; this does not transfer business authority or execution authorization
to the database, or require it to store the tool's executable code.
