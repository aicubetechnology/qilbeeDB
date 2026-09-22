# Contributing to QilbeeDB

Implement each bounded improvement as a feature PR with English code, comments,
commit messages and documentation. Validate observable behavior, isolation,
compatibility and relevant recovery guarantees before publishing the PR. Identify
synthetic fixtures separately from evidence of retrieval or agent-task quality.

## User documentation is part of every validated PR

Maintain the English Markdown guides listed in `docs/user-docs.json`. They are
the source for the Qilbee documentation site's HTML pages. Update the relevant
concept, how-to, reference, example and limitation sections whenever a contract
or user-visible behavior changes. Keep server-owned ranking identities and
experimental status explicit; do not claim unmeasured relevance improvements.

On the integration workstation, export and check the managed Markdown pages:

```bash
python3 scripts/export_user_docs.py \
  --output ../qilbee-site/app/src/doc/qilbeedb
python3 scripts/export_user_docs.py \
  --output ../qilbee-site/app/src/doc/qilbeedb --check
```

Use the absolute sibling-project path when working in an isolated worktree.
`--check` returns nonzero for stale or missing managed pages. The exporter
preserves unrelated site files, rewrites documentation links, and emits title,
description, navigation, release-stage and source-digest frontmatter. It validates
source links and refuses symlink targets. It does not publish the website or
replace its HTML renderer. No site credentials are needed.

The default export marks documentation `unreleased`. After a validated deployment,
use `--status released` for both export and check. Experimental ranking methods
retain their own experimental status even in a released server version. Record
which release and ranking version an evaluation used.

Before creating a PR, build MkDocs, validate OpenAPI and its examples when changed,
export/check the site Markdown, and include evidence in the PR description.
Repeat the export after PR validation so the site sources match the final feature.
The publication workflow merges validated features in batches of five and then
verifies the exact merged image in local Docker.

## Human-centered interfaces

The permanent [interface design requirements](docs/contributing/interface-design.md)
apply to every new or changed interface. Validate the person's complete task,
including accessibility, errors, and recovery, before publishing. Record the
journey and its evidence with the change; a successful API response alone is not
acceptance of the user experience.

## Public user documentation audience

Public user guides serve customers using the managed QilbeeDB platform or running
their own installation. State which path a task applies to; do not require local
installation or bootstrap for platform users. Share API examples through a
configurable base URL and explain credentials without exposing operator secrets.

Keep internal deployment inventories, cloud account or instance details, image
qualification diaries, test-suite counts, team coordination and candidate scan
transcripts out of user guides. Preserve operational evidence in private project
records. Public research reports may document reproducible methodology and limits,
but are separate from the user-guide export. Keep user-relevant compatibility,
recovery limits and security guidance; removing an internal scan transcript does
not justify claiming an image is vulnerability-free.
