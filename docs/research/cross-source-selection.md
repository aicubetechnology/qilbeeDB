# Reproducible cross-source cohort selection

Use this selector after auditing source structure and
[cross-source capacity](cross-source-capacity.md). It creates a manifest of IDs
and roles, not an embedded corpus, retrieval run or agent benchmark. The supported
candidate source is pinned 2Wiki public development data; declared prior fixtures
must match the pinned MuSiQue sources. Unlisted evaluation history remains outside
its knowledge.

## Freeze the plan before selection

The plan requires a nonempty seed, both role names, all four native categories,
explicit role/category order and positive integer quotas for every combination.
There are no default quotas, seeds or automatic retries. Boolean quota values
are rejected. Archive/member/history hashes are bound in the resulting manifest.

The proposed plan format is:

```json
{
  "seed": "your-predeclared-seed",
  "role_order": ["confirmation", "development"],
  "category_order": ["bridge_comparison", "comparison", "compositional", "inference"],
  "quotas": {
    "confirmation": {"bridge_comparison": 30, "comparison": 30, "compositional": 30, "inference": 30},
    "development": {"bridge_comparison": 10, "comparison": 10, "compositional": 10, "inference": 10}
  }
}
```

These counts illustrate feasibility quotas, not a power calculation. Preserve a
hash of the approved plan and candidate identity before invoking the selector.
If a later change alters the formula, selection or exclusion policy, document
and approve a new protocol before examining reserved results.

## Deterministic selection and mutual exclusions

`twowiki_cross_source_greedy_selection_v1` first checks all source integrity and
quarantine rules. It excludes known history, then iterates the declared roles and
categories in order. Candidates are sorted by SHA-256 of the seed, a newline and
the source-qualified query ID, with that ID as tie-break. IDs use `twowiki:` as
a namespace; unrelated dataset component IDs are never equated.

A single accumulating identity set includes history and every selected question,
across all categories and both roles. A selected answer or annotated entity
therefore cannot be reused as either kind in another selected question. Question,
support-title and support-text exclusions also apply. This is greedy selection:
a quota shortfall does not prove no alternative combinatorial selection exists.
It does prevent this frozen protocol from satisfying its quotas. Preserve that
failure; do not reroll the seed or silently lower a quota.

The algorithm does not consult retrieval scores, language-model output or quality
metrics. Answer/support annotations are used only for exclusions. The order and
exclusions affect the distribution, so the resulting cohort is not uniform or
proven statistically independent.

## Select and replay

```bash
python3 scripts/select_cross_source_cohort.py select \
  --archive ./data_ids_april7.zip \
  --musique-source-dir ./musique-source \
  --prior-fixture ./original-source.json \
  --prior-fixture ./fresh-source.json \
  --prior-fixture ./materialized-source.json \
  --plan ./approved-plan.json \
  --output ./selection.json

python3 scripts/select_cross_source_cohort.py verify \
  --archive ./data_ids_april7.zip \
  --musique-source-dir ./musique-source \
  --prior-fixture ./original-source.json \
  --prior-fixture ./fresh-source.json \
  --prior-fixture ./materialized-source.json \
  --output ./selection.json
```

No manifest is written until all quotas succeed. Existing output is never
overwritten. Replay recomputes the entire selection and compares every field;
it takes the plan from the manifest and rejects a separate plan override.
Also compare the manifest and embedded plan against their independently retained
approved hashes: replay establishes internal reproducibility, not authorization
to substitute a different valid plan.

Before a retrieval run, separately freeze corpus materialization, document-only
graph construction, external embeddings, methods, candidate revision, budgets,
metrics and sparse relevance judgments. Keep development and confirmation results
separate. A confirmation role is not the dataset's hidden official test split,
proof of absence from model training or evidence of agent improvement.

## Frozen first cohort

The [selection manifest](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/cross-source-selection/benchmarks/retrieval/twowiki-cross-source-selection-v1.json)
uses seed `qilbeedb-twowiki-cross-source-v1`, the role/category order above and
40 development questions plus 120 confirmation questions. The first attempt
filled all quotas with no seed retry or relaxation. All 160 IDs are source-qualified.
This records successful preparation, not a retrieval-quality result.

The 470-question declared history, official source/alias member hashes, exclusion
projection and both role allocations are bound in the manifest. Before consuming
it, verify its complete replay and compare its plan to the independently retained
approved plan. The original source remains public development data; the
confirmation label describes this experiment's reserved role only.

The [corpus materializer](twowiki-corpus-materialization.md) replays this manifest,
preserves sparse sentence-support provenance and builds source-qualified relations
from documents only. It verifies the complete bundle before reuse.
