#!/usr/bin/env python3
"""Compare a declared RRF grid using development queries only; never publish a profile."""

import argparse
import json
from pathlib import Path
import statistics

from evaluate_retrieval import (
    Client,
    digest,
    metrics,
    prepare,
    query_request,
    save,
    state_lock,
    validate_fixture,
    verify_sources,
)


def fuse_ranks(lexical, semantic, lexical_weight, rank_constant):
    scores = {}
    for candidates, weight in [
        (lexical[:100], lexical_weight),
        (semantic[:100], 1.0 - lexical_weight),
    ]:
        for rank, record_id in enumerate(candidates, 1):
            scores[record_id] = scores.get(record_id, 0.0) + weight / (
                rank_constant + rank
            )
    return sorted(scores, key=lambda record_id: (-scores[record_id], record_id))[:10]


def validate_grid(grid):
    if (
        grid.get("schema_version") != 1
        or grid.get("split") != "development"
        or grid.get("candidate_limit") != 100
        or grid.get("k") != 10
        or not grid.get("rank_constants")
        or not grid.get("lexical_weights")
    ):
        raise ValueError("Unsupported development grid contract")
    if any(type(k) is not int or k <= 0 for k in grid["rank_constants"]):
        raise ValueError("Invalid grid rank constant")
    if any(
        type(w) not in (int, float) or not 0 < w < 1 for w in grid["lexical_weights"]
    ):
        raise ValueError("Invalid grid weight")


def select_profile(fixture, rows, grid, aliases=None):
    development = {
        q["id"]: q for q in fixture["queries"] if q["split"] == "development"
    }
    if not rows or any(row["query_id"] not in development for row in rows):
        raise ValueError("Only development candidates may enter profile selection")
    if {r["query_id"] for r in rows} != set(development) or len(rows) != len(
        development
    ):
        raise ValueError("Development candidate coverage is incomplete or duplicated")
    if (
        grid.get("split") != "development"
        or grid.get("candidate_limit") != 100
        or grid.get("k") != 10
    ):
        raise ValueError("Unsupported development grid contract")
    aliases = aliases or {
        id: id for row in rows for mode in ["lexical", "semantic"] for id in row[mode]
    }

    def summarize(rankings):
        values = [
            metrics([aliases[rid] for rid in ranked], development[row["query_id"]])
            for row, ranked in zip(rows, rankings)
        ]
        return {
            "ndcg_at_10": statistics.mean(
                m["ndcg_at_10"] for m in values if m["ndcg_at_10"] is not None
            ),
            "judged_recall_at_10": statistics.mean(
                m["judged_recall_at_10"]
                for m in values
                if m["judged_recall_at_10"] is not None
            ),
            "per_query_ndcg": {
                row["query_id"]: m["ndcg_at_10"] for row, m in zip(rows, values)
            },
        }

    candidates = []
    for constant in grid["rank_constants"]:
        for weight in grid["lexical_weights"]:
            if type(constant) is not int or constant <= 0 or not 0 < weight < 1:
                raise ValueError("Invalid grid parameter")
            result = summarize(
                [
                    fuse_ranks(row["lexical"], row["semantic"], weight, constant)
                    for row in rows
                ]
            )
            candidates.append(
                dict(
                    result,
                    lexical_weight=weight,
                    semantic_weight=1.0 - weight,
                    rank_constant=constant,
                )
            )
    selected = max(
        candidates,
        key=lambda c: (
            c["ndcg_at_10"],
            c["judged_recall_at_10"],
            -abs(c["lexical_weight"] - 0.5),
            -abs(c["rank_constant"] - 60),
        ),
    )
    return {
        "schema_version": 1,
        "split": "development",
        "queries": len(rows),
        "grid": grid,
        "grid_sha256": digest(grid),
        "selected": selected,
        "candidates": candidates,
        "baselines": {
            mode: summarize([row[mode][:10] for row in rows])
            for mode in ["lexical", "semantic"]
        },
        "selection_bias": "Development scores select a candidate; they are not held-out evidence. Test queries were not executed. No server profile is automatically published or admitted.",
    }


def collect(client, fixture, scope, state_path, cache_path, scan_bytes_limit=67108864):
    validate_fixture(fixture)
    if type(scan_bytes_limit) is not int or not 1 <= scan_bytes_limit <= 268435456:
        raise ValueError("Invalid scan byte budget")
    with state_lock(state_path):
        state, tag = prepare(client, fixture, scope, state_path)
        verify_sources(client, fixture, state, tag)
        frozen = {
            entry["record_id"]: (alias, entry)
            for alias, entry in state["documents"].items()
        }
        expected = {
            "fixture_sha256": digest(fixture),
            "manifest_sha256": digest(state),
            "split": "development",
            "candidate_limit": 100,
        }
        if scan_bytes_limit != 67108864:
            expected["scan_bytes_limit"] = scan_bytes_limit
        cache = (
            json.loads(Path(cache_path).read_text())
            if Path(cache_path).exists()
            else dict(expected, rows=[])
        )
        if any(cache.get(k) != v for k, v in expected.items()):
            raise ValueError("Candidate cache belongs to another frozen trial")
        development = {
            q["id"]: q for q in fixture["queries"] if q["split"] == "development"
        }
        done = {row["query_id"] for row in cache["rows"]}
        if not done <= set(development) or len(done) != len(cache["rows"]):
            raise ValueError("Candidate cache contains reserved or duplicated queries")
        for query in development.values():
            if query["id"] in done:
                continue
            row = {"query_id": query["id"], "counts": {}}
            for mode in ["lexical", "semantic"]:
                path, body = query_request(
                    mode, query, fixture, scope, tag, scan_bytes_limit=scan_bytes_limit
                )
                body["query"]["limit"] = 100
                result, _, _ = client.call("POST", path, body)
                page = result["page"]
                expected_version = "bm25_v1" if mode == "lexical" else "cosine_exact_v1"
                if (
                    result["scope"] != scope
                    or result["ranking_version"] != expected_version
                    or not page["exhaustive"]
                    or page["next_after"] is not None
                ):
                    raise ValueError(
                        "Candidate retrieval scope, version or coverage changed"
                    )
                count = (
                    page["corpus_records"]
                    if mode == "lexical"
                    else page["matched_records"]
                )
                if count != len(frozen):
                    raise ValueError("Candidate corpus coverage changed")
                ids = []
                for hit in page["hits"]:
                    rid = hit["record"]["record_id"]
                    if (
                        rid not in frozen
                        or hit["record"]["revision"] != frozen[rid][1]["revision"]
                    ):
                        raise ValueError(
                            "Candidate source identity or revision changed"
                        )
                    if (
                        mode == "semantic"
                        and hit["embedding"] != frozen[rid][1]["embedding"]
                    ):
                        raise ValueError("Candidate embedding evidence changed")
                    ids.append(rid)
                if len(set(ids)) != len(ids) or len(ids) > 100:
                    raise ValueError("Invalid candidate list")
                row[mode] = ids
                row[mode + "_scores"] = [hit["score"] for hit in page["hits"]]
                row["counts"][mode] = {k: v for k, v in page.items() if k != "hits"}
            cache["rows"].append(row)
            if len(cache["rows"]) % 32 == 0:
                save(cache_path, cache)
                print("Collected development queries:", len(cache["rows"]), flush=True)
        verify_sources(client, fixture, state, tag)
        save(cache_path, cache)
        return cache, {rid: alias for rid, (alias, _) in frozen.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in [
        "fixture",
        "credential-file",
        "scope-file",
        "state",
        "cache",
        "grid",
        "report",
    ]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--base-url", default="http://127.0.0.1:7474")
    parser.add_argument("--scan-bytes-limit", type=int, default=67108864)
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    grid = json.loads(args.grid.read_text())
    validate_grid(grid)
    client = Client(
        args.base_url, json.loads(args.credential_file.read_text())["secret"]
    )
    cache, aliases = collect(
        client,
        fixture,
        json.loads(args.scope_file.read_text()),
        args.state,
        args.cache,
        args.scan_bytes_limit,
    )
    report = select_profile(fixture, cache["rows"], grid, aliases)
    report.update(
        fixture_sha256=digest(fixture),
        manifest_sha256=cache["manifest_sha256"],
        candidates_sha256=digest(cache),
    )
    save(args.report, report)
    print(
        "Development selection:",
        {k: v for k, v in report["selected"].items() if k != "per_query_ndcg"},
    )


if __name__ == "__main__":
    main()
