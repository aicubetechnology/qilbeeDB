# Feature delivery and merge batches

Code, documentation, code comments, commit messages and pull request text must
be written in English.

Publish each improvement as a separate feature pull request with a concrete
behavioral contract, compatibility notes and reproducible validation. A feature
is complete only when its published commit passes the relevant checks. Preserve
unrelated local work and validate in a clean checkout when the working tree
contains changes outside the feature.

## Validation record

For each feature, record the problem, expected behavior, tests and remaining
limits. Add a failing regression test before fixing a behavioral defect. Report
the actual commands, results and commit tested. Keep scientific hypotheses
separate from measured outcomes, and link primary sources for research-derived
design decisions. Do not advertise a Rust library capability as an HTTP or SDK
capability until that integration exists and is tested.

Run the relevant tests and the workspace suite before publication. Build the
documentation when changing it. Report pre-existing lint failures explicitly;
they must not be described as passing checks. Unit tests and synthetic examples
do not establish comparative benchmark leadership.

## Five-PR batches

Merge into `main` after each batch of five validated feature PRs. For dependent
changes, use stacked branches so each PR displays only its own improvement:

1. Base the first feature on `main` and each dependent feature on its predecessor.
2. Publish all five PRs with their own validation results and dependency notes.
3. Verify the final batch in a clean checkout, and inspect remote check results.
4. Merge in dependency order. Retarget each remaining PR to `main` only after
   its predecessor is merged. Preserve shared ancestry with merge commits.
5. Verify the merged tree matches the tested tree. If integration changes the
   tree, run the affected validation again before declaring the batch complete.

A failed check, unresolved conflict or incompatible integration must be fixed
before merging. Keep unrelated work outside the batch and never force-push the
principal branch.

## Authorized review exception

The project maintainer has authorized technical self-review and a temporary
review-rule exception for validated five-PR batches. GitHub does not accept an
author's review as an independent approval. Record the technical review and this
exception transparently in each PR; never represent it as an independent review.

If the required-review rule blocks an otherwise validated batch, save the
ruleset configuration and temporarily add only the authenticated maintainer as
a bypass actor with `pull_request` mode. Preserve every other rule, merge the
five reviewed commits in dependency order, then remove the temporary exception
immediately. Restore the rule on failure as well, and verify the resulting
configuration. Do not disable enforcement, permit direct pushes, bypass failed
tests or leave the exception enabled between batches. If the ruleset changes
concurrently, preserve those changes while removing this batch's exception.
