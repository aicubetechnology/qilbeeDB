#!/usr/bin/env python3
"""Compare two immutable hybrid profiles with shared, interleaved HTTP baselines."""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import json
from pathlib import Path
import random
import time

from evaluate_retrieval import (
    Client,
    PROFILES,
    container_resources,
    digest,
    metrics,
    paired_interval,
    query_request,
    save,
    state_lock,
    summarize,
    validate_fixture,
    validate_state,
    verify_sources,
)

METHODS = ("lexical", "semantic", "weighted_rrf_v1", "weighted_rrf_v2")


def make_plan(fixture, state, scan_bytes_limit, concurrency, seed):
    if type(scan_bytes_limit) is not int or not 1 <= scan_bytes_limit <= 268435456:
        raise ValueError("Invalid scan byte budget")
    if type(concurrency) is not int or not 1 <= concurrency <= 64:
        raise ValueError("Invalid concurrency")
    return {
        "schema_version": 1,
        "fixture_sha256": digest(fixture),
        "manifest_sha256": digest(state),
        "split": "test",
        "k": 10,
        "scan_limit": 10000,
        "scan_bytes_limit": scan_bytes_limit,
        "client_concurrency": concurrency,
        "seed": seed,
        "repetitions": 1,
        "warmup": "first development query, once per method, excluded from measurements",
        "profiles": {v: PROFILES[v] for v in METHODS if v in PROFILES},
        "order": "one seeded shuffle of all query/method jobs; shared lexical and semantic baselines",
        "cache": "RocksDB/OS caches not flushed; no server retrieval-result cache",
    }


def validate_page(result, scope, frozen, method):
    page = result["page"]
    expected = {"lexical": "bm25_v1", "semantic": "cosine_exact_v1"}.get(method, method)
    if result["scope"] != scope or result["ranking_version"] != expected:
        raise ValueError("Scope or ranking version differs")
    if page["exhaustive"] is not True or page["next_after"] is not None:
        raise ValueError("Incomplete corpus scan")
    count = page["matched_records"] if method == "semantic" else page["corpus_records"]
    if count != len(frozen):
        raise ValueError("Frozen corpus coverage differs")
    if method in PROFILES and (
        page["ranking"] != PROFILES[method]
        or page["embedding_coverage"] != "complete"
        or page["embedded_records"] != len(frozen)
    ):
        raise ValueError("Hybrid profile or current embedding coverage differs")
    ids = [h["record"]["record_id"] for h in page["hits"]]
    if len(ids) > 10 or len(ids) != len(set(ids)) or not set(ids) <= set(frozen):
        raise ValueError("Duplicate, excessive or out-of-corpus results")
    for hit in page["hits"]:
        entry = frozen[hit["record"]["record_id"]][1]
        if hit["record"]["revision"] != entry["revision"]:
            raise ValueError("Stale source revision")
        if hit.get("embedding") and hit["embedding"] != entry["embedding"]:
            raise ValueError("Embedding evidence differs")
    return [frozen[rid][0] for rid in ids]


def evaluate(base_url, secret, fixture, state, plan, container=None, progress=None):
    validate_fixture(fixture)
    validate_state(state, fixture)
    expected = make_plan(
        fixture,
        state,
        plan["scan_bytes_limit"],
        plan["client_concurrency"],
        plan["seed"],
    )
    if plan != expected:
        raise ValueError(
            "Pinned plan differs from the frozen corpus or supported profiles"
        )
    if len(state["documents"]) != len(fixture["documents"]):
        raise ValueError("Prepare a complete source manifest before running the trial")
    client = Client(base_url, secret)
    scope = state["scope"]
    tag = "retrieval-fixture-" + plan["fixture_sha256"]
    frozen = {
        entry["record_id"]: (alias, entry)
        for alias, entry in state["documents"].items()
    }
    verify_sources(client, fixture, state, tag)
    health, _, _ = client.call("GET", "/health")
    catalog, _, _ = client.call("GET", "/api/v1/memory/ranking-profiles")
    available = {p["version"]: p for p in catalog["hybrid_profiles"]}
    if any(available.get(v) != p for v, p in plan["profiles"].items()):
        raise ValueError("Server profile definitions differ from the pinned plan")
    if (
        catalog["execution_limits"]["max_scan_bytes"] < plan["scan_bytes_limit"]
        or catalog["execution_limits"]["max_concurrent_retrievals"]
        < plan["client_concurrency"]
    ):
        raise ValueError("Server capacity does not admit the pinned trial")

    def one(query, method):
        mode = method if method in ("lexical", "semantic") else "hybrid"
        path, body = query_request(
            mode,
            query,
            fixture,
            scope,
            tag,
            method if mode == "hybrid" else "weighted_rrf_v1",
            plan["scan_bytes_limit"],
        )
        result, elapsed, size = Client(base_url, secret).call("POST", path, body)
        ranked = validate_page(result, scope, frozen, method)
        page = result["page"]
        return {
            "query_id": query["id"],
            "split": query["split"],
            "category": query["category"],
            "method": method,
            "ranked": ranked,
            "metrics": metrics(ranked, query),
            "counts": {k: v for k, v in page.items() if k not in ("hits", "ranking")},
            "ranking_profile": page.get("ranking"),
            "scores": [
                {k: v for k, v in hit.items() if k in ("score", "lexical", "semantic")}
                for hit in page["hits"]
            ],
            "samples": [
                {
                    "retrieval_ms": result["timing"]["retrieval_micros"] / 1000,
                    "http_ms": elapsed,
                    "response_bytes": size,
                }
            ],
        }

    warmup = next(q for q in fixture["queries"] if q["split"] == "development")
    for method in METHODS:
        one(warmup, method)
    queries = [q for q in fixture["queries"] if q["split"] == "test"]
    jobs = [(q, method) for q in queries for method in METHODS]
    random.Random(plan["seed"]).shuffle(jobs)
    before = container_resources(container) if container else {"available": False}
    started = time.perf_counter()
    rows, failures = [], []
    with ThreadPoolExecutor(max_workers=plan["client_concurrency"]) as pool:
        futures = {pool.submit(one, q, method): (q["id"], method) for q, method in jobs}
        for future in as_completed(futures):
            qid, method = futures[future]
            try:
                rows.append(future.result())
            except Exception as error:
                failures.append(
                    {
                        "query_id": qid,
                        "method": method,
                        "error_type": type(error).__name__,
                    }
                )
            completed = len(rows) + len(failures)
            if completed % 40 == 0:
                print(
                    f"Completed {completed}/{len(jobs)} query/method pairs; failures {len(failures)}",
                    flush=True,
                )
                if progress:
                    save(
                        progress,
                        {
                            "trial_plan_sha256": digest(plan),
                            "completed": completed,
                            "total": len(jobs),
                            "failures": failures,
                        },
                    )
    duration = time.perf_counter() - started
    after = container_resources(container) if container else {"available": False}
    try:
        verify_sources(client, fixture, state, tag)
    except Exception as error:
        failures.append(
            {"stage": "final_source_verification", "error_type": type(error).__name__}
        )
    summaries = {
        method: summarize([r for r in rows if r["method"] == method])
        for method in METHODS
    }
    by_key = {(r["query_id"], r["method"]): r for r in rows}
    valid = not failures and len(by_key) == len(jobs) == len(rows)
    paired = {}
    if valid:
        for candidate in ("weighted_rrf_v1", "weighted_rrf_v2"):
            for baseline in METHODS:
                if baseline == candidate or (
                    candidate == "weighted_rrf_v1" and baseline == "weighted_rrf_v2"
                ):
                    continue
                changes = []
                for q in queries:
                    left, right = (
                        by_key[(q["id"], candidate)],
                        by_key[(q["id"], baseline)],
                    )
                    a, b = left["metrics"]["ndcg_at_10"], right["metrics"]["ndcg_at_10"]
                    if a is not None and b is not None:
                        changes.append(
                            {
                                "query_id": q["id"],
                                "delta": a - b,
                                "candidate": left["ranked"],
                                "baseline": right["ranked"],
                            }
                        )
                paired[candidate + "_minus_" + baseline] = {
                    "interval": paired_interval(
                        [r["delta"] for r in changes], plan["seed"]
                    ),
                    "wins": sum(r["delta"] > 0 for r in changes),
                    "losses": sum(r["delta"] < 0 for r in changes),
                    "ties": sum(r["delta"] == 0 for r in changes),
                    "changes": changes,
                }
    return {
        "schema_version": 1,
        "trial_plan": plan,
        "trial_plan_sha256": digest(plan),
        "server_health": health,
        "execution_limits": catalog["execution_limits"],
        "model_space": fixture["space"],
        "source_revision_count": len(frozen),
        "valid_comparison": valid,
        "failures": failures,
        "summary": summaries,
        "paired": paired,
        "rows": sorted(rows, key=lambda r: (r["query_id"], r["method"])),
        "resources": {
            "before": before,
            "after": after,
            "elapsed_seconds": duration,
            "attribution": "Shared interleaved campaign; no per-method CPU attribution",
        },
        "limitations": [
            "One observation per query/method; no repeated-query stability estimate.",
            "HTTP and engine timing exclude external generation; frozen vectors reused without provider calls.",
            "Sparse external qrels: judged recall is not exhaustive recall.",
            "Development-selected profile; exploratory bootstrap without multiplicity correction.",
            "No abstention, agent-task, multi-domain or production admission claim.",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("fixture", "state", "plan"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--write-plan", action="store_true")
    parser.add_argument("--scan-bytes-limit", type=int, default=134217728)
    parser.add_argument("--concurrency", type=int, default=2)
    parser.add_argument("--seed", type=int, default=20260920)
    parser.add_argument("--credential-file", type=Path)
    parser.add_argument("--base-url", default="http://127.0.0.1:7474")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--container")
    args = parser.parse_args()
    with state_lock(args.state):
        fixture, state = json.loads(args.fixture.read_text()), json.loads(
            args.state.read_text()
        )
        if args.write_plan:
            if args.plan.exists():
                parser.error("Plan already exists")
            save(
                args.plan,
                make_plan(
                    fixture, state, args.scan_bytes_limit, args.concurrency, args.seed
                ),
            )
            print("Wrote interleaved plan:", args.plan)
            return 0
        if not args.report or not args.credential_file:
            parser.error("A run requires --report and --credential-file")
        if args.report.exists():
            parser.error("Report already exists; preserve completed evidence")
        plan = json.loads(args.plan.read_text())
        if (args.scan_bytes_limit, args.concurrency, args.seed) != (
            plan["scan_bytes_limit"],
            plan["client_concurrency"],
            plan["seed"],
        ):
            parser.error("Execution arguments differ from the pinned plan")
        result = evaluate(
            args.base_url,
            json.loads(args.credential_file.read_text())["secret"],
            fixture,
            state,
            plan,
            args.container,
            args.report.with_suffix(".progress.json"),
        )
        save(args.report, result)
        print("Wrote interleaved comparison; valid:", result["valid_comparison"])
        return 0 if result["valid_comparison"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
