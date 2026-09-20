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
