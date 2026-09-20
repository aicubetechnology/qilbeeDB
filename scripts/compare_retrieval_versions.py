#!/usr/bin/env python3
"""Compare two frozen hybrid reports without pooling incompatible trials."""

import argparse
import json
from pathlib import Path

from evaluate_retrieval import digest, paired_interval, save


def compare(before, after):
    for key in [
        "fixture_sha256",
        "manifest_sha256",
        "scope_sha256",
        "source_revisions",
        "model_space",
    ]:
        if before[key] != after[key]:
            raise ValueError("Reports do not share the same frozen corpus and scope")
    for report in [before, after]:
        if not report["valid_comparison"] or report["conditions"][
            "selected_split"
        ] not in ("all", "test"):
            raise ValueError("Version comparison requires valid reserved-query reports")
    old_plan, new_plan = dict(before["trial_plan"]), dict(after["trial_plan"])
    old_profile, new_profile = old_plan.pop("hybrid_profile"), new_plan.pop(
        "hybrid_profile"
    )
    if old_plan != new_plan:
        raise ValueError("Trial conditions differ beyond the selected hybrid profile")
    old = {
        r["query_id"]: r
        for r in before["rows"]
        if r["split"] == "test" and r["mode"] == "hybrid"
    }
    new = {
        r["query_id"]: r
        for r in after["rows"]
        if r["split"] == "test" and r["mode"] == "hybrid"
    }
    if not old or set(old) != set(new):
        raise ValueError("Reserved query coverage differs")
    # The independent baseline rankings must remain identical between trials.
    for mode in ["lexical", "semantic"]:
        baseline_before = {
            r["query_id"]: r["ranked"]
            for r in before["rows"]
            if r["mode"] == mode and r["split"] == "test"
        }
        baseline_after = {
            r["query_id"]: r["ranked"]
            for r in after["rows"]
            if r["mode"] == mode and r["split"] == "test"
        }
        if set(baseline_before) != set(old) or baseline_before != baseline_after:
            raise ValueError("A baseline ranking changed between version trials")
    changes = [
        {
            "query_id": qid,
            "category": new[qid]["category"],
            "before": old[qid]["metrics"]["ndcg_at_10"],
            "after": new[qid]["metrics"]["ndcg_at_10"],
            "before_ranking": old[qid]["ranked"],
            "after_ranking": new[qid]["ranked"],
        }
        for qid in sorted(old)
        if old[qid]["metrics"]["ndcg_at_10"] is not None
        and new[qid]["metrics"]["ndcg_at_10"] is not None
    ]
    for row in changes:
        row["delta"] = row["after"] - row["before"]
    return {
        "schema_version": 1,
        "before_profile": old_profile,
        "after_profile": new_profile,
        "before_report_sha256": digest(before),
        "after_report_sha256": digest(after),
        "fixture_sha256": before["fixture_sha256"],
        "manifest_sha256": before["manifest_sha256"],
        "paired_ndcg_delta": paired_interval(
            [r["delta"] for r in changes], old_plan["seed"]
        ),
        "wins": sum(r["delta"] > 0 for r in changes),
        "losses": sum(r["delta"] < 0 for r in changes),
        "ties": sum(r["delta"] == 0 for r in changes),
        "changes": changes,
        "interpretation": "Exploratory paired bootstrap, not corrected for multiple comparisons. Preserve every loss; no automatic production admission or agent-quality claim.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["before", "after", "report"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    save(
        args.report,
        compare(
            json.loads(args.before.read_text()), json.loads(args.after.read_text())
        ),
    )
    print("Wrote paired version comparison:", args.report)


if __name__ == "__main__":
    main()
