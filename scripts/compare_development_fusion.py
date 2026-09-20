#!/usr/bin/env python3
"""Reproduce development-only RRF and normalized-score fusion comparisons offline."""

import argparse
import json
import math
from pathlib import Path
import statistics
from evaluate_retrieval import digest, metrics, save, validate_fixture, validate_state
from tune_retrieval import select_profile, validate_grid


def normalized_ranking(row, weight):
    fused = {}
    for channel, alpha in (("lexical", weight), ("semantic", 1.0 - weight)):
        ids, scores = row[channel], row[channel + "_scores"]
        if len(ids) != len(scores) or len(ids) > 100 or len(set(ids)) != len(ids):
            raise ValueError("Invalid channel candidates")
        if any(type(s) not in (float, int) or not math.isfinite(s) for s in scores):
            raise ValueError("Channel scores must be finite")
        if not scores:
            continue
        lo, hi = min(scores), max(scores)
        for rid, score in zip(ids, scores):
            value = (score - lo) / (hi - lo) if hi > lo else 1.0
            fused[rid] = fused.get(rid, 0.0) + alpha * value
    return sorted(fused, key=lambda rid: (-fused[rid], rid))[:10]


def compare(fixture, cache, state, grid):
    validate_fixture(fixture)
    validate_state(state, fixture)
    validate_grid(grid)
    if cache["fixture_sha256"] != digest(fixture) or cache["manifest_sha256"] != digest(
        state
    ):
        raise ValueError(
            "Candidate cache does not match the frozen fixture and manifest"
        )
    aliases = {v["record_id"]: k for k, v in state["documents"].items()}
    rrf = select_profile(fixture, cache["rows"], grid, aliases)
    queries = {q["id"]: q for q in fixture["queries"] if q["split"] == "development"}
    candidates = []
    for weight in grid["lexical_weights"]:
        values = [
            metrics(
                [aliases[r] for r in normalized_ranking(row, weight)],
                queries[row["query_id"]],
            )
            for row in cache["rows"]
        ]
        candidates.append(
            {
                "method": "candidate_minmax",
                "lexical_weight": weight,
                "semantic_weight": 1 - weight,
                "ndcg_at_10": statistics.mean(
                    v["ndcg_at_10"] for v in values if v["ndcg_at_10"] is not None
                ),
                "judged_recall_at_10": statistics.mean(
                    v["judged_recall_at_10"]
                    for v in values
                    if v["judged_recall_at_10"] is not None
                ),
                "per_query_ndcg": {
                    row["query_id"]: v["ndcg_at_10"]
                    for row, v in zip(cache["rows"], values)
                },
            }
        )
    selected = max(
        candidates,
        key=lambda c: (
            c["ndcg_at_10"],
            c["judged_recall_at_10"],
            -abs(c["lexical_weight"] - 0.5),
        ),
    )
    return {
        "schema_version": 1,
        "split": "development",
        "fixture_sha256": digest(fixture),
        "candidates_sha256": digest(cache),
        "rrf": rrf,
        "normalized": {"selected": selected, "candidates": candidates},
        "normalization": "Per-channel min-max on its scoped top-100 candidate list; equal range contributes 1; missing channel contributes 0. No raw BM25/cosine addition. UUID ascending tie-break.",
        "limits": "Development selection only. No test queries, HTTP execution, automatic profile publication or evidence of general superiority.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("fixture", "cache", "state", "grid", "report"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        parser.error("Report already exists")
    values = [
        json.loads(getattr(args, name).read_text())
        for name in ("fixture", "cache", "state", "grid")
    ]
    save(args.report, compare(*values))
    print("Wrote development fusion comparison:", args.report)


if __name__ == "__main__":
    main()
