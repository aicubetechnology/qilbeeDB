#!/usr/bin/env python3
"""Prepare or compare frozen native retrieval and explicit typed-graph methods.

Run only against an authorized isolated benchmark scope. Import is explicit;
evaluation is read-only, never retries failed calls and preserves partial evidence.
"""

import argparse
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess
import threading
import time

from evaluate_retrieval import (
    Client,
    PROFILES,
    canonical,
    digest,
    metrics,
    paired_interval,
    prepare,
    quantile,
    query_request,
    save,
    state_lock,
    validate_state,
    verify_sources,
)
from graph_evaluation_contract import (
    fences,
    seed_version,
    graph_profile,
    prepare_relations,
    validate_page,
    verify_fixture_graph,
    verify_relations,
)


def request_for(method, query, fixture, state, protocol):
    tag = "retrieval-fixture-" + state["fixture_sha256"]
    if not method.startswith("graph_"):
        mode = "hybrid" if method in PROFILES else method
        return query_request(
            mode,
            query,
            fixture,
            state["scope"],
            tag,
            method if mode == "hybrid" else "weighted_rrf_v1",
            protocol["scan_bytes_limit"],
        )
    seed = {"mode": "lexical", "text": query["text"]}
    if method != "graph_lexical_balanced":
        seed.update(
            mode="hybrid",
            space=fixture["space"],
            vector=query["vector"],
            ranking_version=seed_version(protocol, method),
            min_score=protocol["min_score"],
        )
    expansion = dict(protocol["expansion"])
    if method.endswith("depth_zero"):
        expansion["max_depth"] = 0
    return "/api/v1/memory/search/graph", {
        "contract_version": 1,
        "scope": state["scope"],
        "query": {
            "ranking_version": protocol["graph_profiles"][method],
            "seed": seed,
            "limit": protocol["k"],
            "scan_limit": protocol["scan_limit"],
            "scan_bytes_limit": protocol["scan_bytes_limit"],
            "tag": tag,
            "expansion": expansion,
        },
    }


def evaluation_stage(protocol):
    stages = {
        "graph_multihop_compare_v1": "test",
        "graph_balanced_v1_support_reserved_v1": "test",
        "graph_seed_matrix_development_v1": "development",
        "graph_path_strength_development_v1": "development",
        "twowiki_strength_development_v1": "development",
        "twowiki_captured1536_development_v1": "development",
        "twowiki_captured1536_reserved_v1": "test",
        "graph_path_strength_regression_v1": "test",
        "graph_multihop_development_v1": "development",
        "graph_base_preserving_development_v1": "development",
        "graph_best_channel_development_v1": "development",
        "graph_best_channel_v1_seed_development_v1": "development",
        "graph_base_preserving_reserved_v1": "test",
    }
    expected = stages.get(protocol.get("protocol_version"))
    if expected is None or protocol.get("split") != expected:
        raise ValueError("Protocol identity and evaluation split differ")
    if protocol["protocol_version"] == "graph_path_strength_regression_v1":
        return "observed_regression"
    return "development" if expected == "development" else "reserved_comparison"


def evaluation_queries(fixture, protocol):
    evaluation_stage(protocol)
    queries = {q["id"]: q for q in fixture["queries"] if q["split"] == protocol["split"]}
    if not queries:
        raise ValueError("The declared evaluation split has no queries")
    return queries


def verify_protocol(protocol, fixture, graph):
    verify_fixture_graph(fixture, graph)
    evaluation_queries(fixture, protocol)
    if (
        protocol["source_sha256"] != graph["source_sha256"]
        or protocol["relations_sha256"] != graph["relations_sha256"]
        or protocol["graph_policy_sha256"] != graph["policy_sha256"]
        or protocol["k"] != 10
        or protocol["scan_limit"] != 10000
        or protocol["min_score"] != -1
        or protocol["client_concurrency"] != 1
        or type(protocol["repetitions"]) is not int
        or not 1 <= protocol["repetitions"] <= 20
    ):
        raise ValueError("Protocol differs from supported frozen comparison")
    if protocol["protocol_version"] in ("twowiki_strength_development_v1", "twowiki_captured1536_development_v1", "twowiki_captured1536_reserved_v1"):
        from twowiki_evaluation_contract import verify_twowiki_protocol
        verify_twowiki_protocol(protocol, fixture, graph)
        return
    if protocol["protocol_version"] in ("graph_path_strength_development_v1", "graph_path_strength_regression_v1"):
        from path_strength_contract import verify_path_strength_protocol
        verify_path_strength_protocol(protocol, fixture)
        return
    if protocol["protocol_version"] == "graph_balanced_v1_support_reserved_v1":
        from support_reserved_contract import verify_support_reserved
        verify_support_reserved(protocol, fixture, graph)
        return
    if protocol["protocol_version"] == "graph_seed_matrix_development_v1":
        profiles = {}
        seeds = {}
        for version in (1, 2):
            for profile in ("balanced", "entity", "best_channel", "depth_zero"):
                method = f"graph_v{version}_{profile}"
                profiles[method] = f"typed_path_{'balanced' if profile == 'depth_zero' else profile}_v1"
                seeds[method] = f"weighted_rrf_v{version}"
        expected_methods = {"lexical", "semantic", "weighted_rrf_v1", "weighted_rrf_v2"} | set(profiles)
        if (set(protocol["methods"]) != expected_methods
            or len(protocol["methods"]) != len(expected_methods)
            or protocol["graph_profiles"] != profiles
            or protocol.get("graph_seeds") != seeds
            or "graph_seed_hybrid_version" in protocol
            or protocol["primary_comparison"] != {"candidate": "graph_v1_balanced", "baseline": "weighted_rrf_v1"}
            or protocol.get("seed_comparisons") != [
                {"candidate": f"graph_v1_{profile}", "baseline": f"graph_v2_{profile}"}
                for profile in ("balanced", "entity", "best_channel")]
            or protocol["default_admission"] is not False):
            raise ValueError("Seed matrix differs from the frozen per-arm comparison")
        return
    if "graph_seeds" in protocol or "seed_comparisons" in protocol:
        raise ValueError("Legacy protocol cannot override seeds per arm")
    required = {
        "lexical",
        "semantic",
        "weighted_rrf_v1",
        "weighted_rrf_v2",
        "graph_hybrid_balanced",
        "graph_hybrid_entity",
        "graph_lexical_balanced",
        "graph_hybrid_depth_zero",
    }
    v1_seed_trial = protocol["protocol_version"] == "graph_best_channel_v1_seed_development_v1"
    best_channel_trial = v1_seed_trial or protocol["protocol_version"] == "graph_best_channel_development_v1"
    candidate_trial = best_channel_trial or protocol["protocol_version"] in (
        "graph_base_preserving_development_v1", "graph_base_preserving_reserved_v1"
    )
    if candidate_trial:
        required.add("graph_hybrid_base_preserving")
    if best_channel_trial:
        required.add("graph_hybrid_best_channel")
    if set(protocol["methods"]) != required or len(protocol["methods"]) != len(
        required
    ):
        raise ValueError("Unexpected or missing methods")
    profiles = {
        method: (
            "typed_path_entity_v1"
            if method == "graph_hybrid_entity"
            else "typed_path_balanced_v1"
        )
        for method in required
        if method.startswith("graph_")
    }
    candidate = "graph_hybrid_balanced"
    if candidate_trial:
        profiles["graph_hybrid_base_preserving"] = "typed_path_base_preserving_v1"
        candidate = "graph_hybrid_base_preserving"
    if best_channel_trial:
        profiles["graph_hybrid_best_channel"] = "typed_path_best_channel_v1"
        candidate = "graph_hybrid_best_channel"
    expected_seed = "weighted_rrf_v1" if v1_seed_trial else "weighted_rrf_v2"
    if (
        protocol["graph_profiles"] != profiles
        or protocol["graph_seed_hybrid_version"] != expected_seed
        or protocol["primary_comparison"]
        != {"candidate": candidate, "baseline": expected_seed}
        or protocol["default_admission"] is not False
    ):
        raise ValueError("Method label, seed profile or declared comparison changed")


def verify_seed_baselines(rows, queries, protocol):
    """Bind every graph arm's anchors and depth-zero control to its seed."""
    for qid in queries:
        for method in protocol["graph_profiles"]:
            baseline = "lexical" if method == "graph_lexical_balanced" else seed_version(protocol, method)
            if method.endswith("depth_zero") and rows[(qid, method)]["ranked"] != rows[(qid, baseline)]["ranked"]:
                raise ValueError("Depth-zero ablation changed base result order")
            expected = [h["record"]["record_id"] for h in rows[(qid, baseline)]["hits"][:4]]
            if [r["record_id"] for r in rows[(qid, method)]["work"]["seed"]["selected_anchors"]] != expected:
                raise ValueError("Graph anchors differ from independently retrieved baseline")


class ProcessMonitor:
    """Optional whole-process observations; not per-method CPU attribution."""

    def __init__(self, pid):
        self.pid = pid
        self.samples = []
        self.errors = []
        self.stop = threading.Event()
        self.worker = None

    def sample(self):
        if not self.pid:
            return
        try:
            output = subprocess.check_output(
                ["ps", "-o", "rss=,time=", "-p", str(self.pid)], text=True, timeout=2
            ).strip()
            rss, cpu = output.split()
            parts = cpu.split(":")
            if len(parts) not in (2, 3) or "-" in cpu:
                raise ValueError("Unsupported process CPU format")
            seconds = sum(float(v) * 60**i for i, v in enumerate(reversed(parts)))
            self.samples.append(
                {
                    "observed_at": time.time(),
                    "rss_bytes": int(rss) * 1024,
                    "cpu_seconds": seconds,
                }
            )
        except Exception as exc:
            self.errors.append(type(exc).__name__)

    def start(self):
        self.sample()

        def poll():
            while not self.stop.wait(1):
                self.sample()

        if self.pid:
            self.worker = threading.Thread(target=poll, daemon=True)
            self.worker.start()

    def finish(self):
        self.stop.set()
        if self.worker:
            self.worker.join(timeout=3)
        self.sample()
        return {
            "available": bool(self.samples),
            "errors": self.errors,
            "samples": self.samples,
            "peak_sampled_rss_bytes": max(
                (s["rss_bytes"] for s in self.samples), default=None
            ),
            "cpu_seconds_delta": (
                self.samples[-1]["cpu_seconds"] - self.samples[0]["cpu_seconds"]
                if len(self.samples) > 1
                else None
            ),
            "scope": "entire server process during measured requests; one-second RSS sampling, OS process CPU counter; no per-method attribution",
        }


def summary(rows):
    metrics_keys = [
        "ndcg_at_10",
        "judged_recall_at_10",
        "all_labeled_supports_at_10",
        "no_useful_result",
    ]
    return {
        "queries": len(rows),
        **{k: statistics.mean(r["metrics"][k] for r in rows) for k in metrics_keys},
        **{
            k: {
                label: (None if any(s[k] is None for r in rows for s in r["samples"])
                        else quantile([s[k] for r in rows for s in r["samples"]], p))
                for label, p in [("p50", 0.5), ("p95", 0.95)]
            }
            for k in [
                "retrieval_ms",
                "http_ms",
                "embedding_plus_http_ms",
                "response_bytes",
                "payload_bytes",
            ]
        },
        "graph_cut_query_fraction": statistics.mean(
            not r["coverage"].get("graph_complete", True) for r in rows
        ),
        "candidate_cut_query_fraction": statistics.mean(
            not r["coverage"]["candidates_complete"] for r in rows
        ),
    }


def validate_generation_timings(generation, fixture, protocol):
    """Validate existing per-query timing evidence before any benchmark HTTP call."""
    if generation.get("fixture_sha256") != digest(fixture):
        raise ValueError("Embedding timings belong to another frozen vector set")
    if all(method in ("lexical", "graph_lexical_balanced") for method in protocol["methods"]):
        return
    if "generation_sha256" in protocol and digest(generation) != protocol["generation_sha256"]:
        raise ValueError("Generation evidence differs from the frozen protocol")
    if "query_timing_status" in protocol and generation.get("query_timing_status") != protocol["query_timing_status"]:
        raise ValueError("Generation timing availability differs from the frozen protocol")
    if generation.get("query_timing_status") == "unavailable_batch_capture":
        if generation.get("queries") != {} or not generation.get("batch_evidence"):
            raise ValueError("Unavailable batch timing requires separate evidence and no invented query allocation")
        return
    if generation.get("query_timing_status") not in (None, "measured"):
        raise ValueError("Unknown query timing status")
    timings = generation.get("queries")
    if not isinstance(timings, dict):
        raise ValueError("Per-query embedding timings are unavailable; batch timing cannot be allocated to queries")
    for query_id in evaluation_queries(fixture, protocol):
        entry = timings.get(query_id)
        if not isinstance(entry, dict) or "elapsed_ms" not in entry:
            raise ValueError("Missing embedding timing for a measured query")
        value = entry["elapsed_ms"]
        try:
            valid = type(value) in (int, float) and math.isfinite(value) and value >= 0
        except OverflowError:
            valid = False
        if not valid:
            raise ValueError("Embedding timing must be a finite nonnegative duration")


def evaluate(
    client,
    fixture,
    graph,
    state,
    protocol,
    plan_path,
    report_path,
    generation,
    pid=None,
):
    verify_protocol(protocol, fixture, graph)
    validate_state(state, fixture)
    if state["fixture_sha256"] != digest(fixture):
        raise ValueError("Manifest belongs to another frozen vector set")
    validate_generation_timings(generation, fixture, protocol)
    identity = client.call("GET", "/api/v1/identity")[0]["credential"]
    if (
        identity["tenant_id"] != state["tenant_id"]
        or identity["spec"]["subject_id"] != state["subject_id"]
    ):
        raise ValueError("Credential identity changed")
    tag = "retrieval-fixture-" + state["fixture_sha256"]
    verify_sources(client, fixture, state, tag)
    verify_relations(client, graph, state)
    health = client.call("GET", "/health")[0]
    catalog = client.call("GET", "/api/v1/memory/graph-ranking-profiles")[0]
    available = {p["version"]: p for p in catalog["graph_profiles"]}
    for version in set(protocol["graph_profiles"].values()):
        if available.get(version) != graph_profile(version):
            raise ValueError("Server graph profile differs")
    old_catalog = client.call("GET", "/api/v1/memory/ranking-profiles")[0]
    if any(
        {p["version"]: p for p in old_catalog["hybrid_profiles"]}.get(v) != p
        for v, p in PROFILES.items()
    ):
        raise ValueError("Server hybrid profile differs")
    before = fences(client, state["scope"])
    plan = {
        "schema_version": 1,
        "protocol_sha256": digest(protocol),
        "fixture_sha256": digest(fixture),
        "graph_sha256": digest(graph),
        "manifest_sha256": digest(state),
        "generation_sha256": digest(generation),
        "server_health": health,
        "execution_limits": catalog["execution_limits"],
        "fences": before,
        "scope": state["scope"],
        "tenant_id": state["tenant_id"],
        "subject_id": state["subject_id"],
        "model_space": fixture["space"],
        "protocol": protocol,
    }
    if Path(plan_path).exists():
        if json.loads(Path(plan_path).read_text()) != plan:
            raise ValueError("Pinned plan changed")
    else:
        save(plan_path, plan)
    if Path(report_path).exists():
        raise ValueError("Preserve prior report, including failed/partial observations")
    report = {
        "schema_version": 1,
        "status": "running",
        "evaluation_stage": evaluation_stage(protocol),
        "started_at": datetime.now(timezone.utc).isoformat(),
        "plan": plan,
        "rows": [],
        "failures": [],
        "environment": {
            "system": platform.system(),
            "architecture": platform.machine(),
            "python": platform.python_version(),
            "load_average": os.getloadavg(),
        },
        "scope_of_evidence": protocol["scope_of_evidence"],
        "default_admission": False,
    }
    save(report_path, report)
    monitor = ProcessMonitor(pid)
    rows = {}
    try:
        warmup = min(
            (q for q in fixture["queries"] if q["split"] == "development"),
            key=lambda q: q["id"],
        )
        for method in protocol["methods"]:
            route, body = request_for(method, warmup, fixture, state, protocol)
            result, _, _ = client.call("POST", route, body)
            validate_page(result, fixture, state, warmup, method, protocol)
        queries = evaluation_queries(fixture, protocol)
        jobs = [
            (qid, method, rep)
            for qid in sorted(queries)
            for method in protocol["methods"]
            for rep in range(protocol["repetitions"])
        ]
        random.Random(protocol["seed"]).shuffle(jobs)
        monitor.start()
        for index, (qid, method, rep) in enumerate(jobs):
            query = queries[qid]
            route, body = request_for(method, query, fixture, state, protocol)
            result, elapsed, byte_count = client.call("POST", route, body)
            ranked = validate_page(result, fixture, state, query, method, protocol)
            page = result["page"]
            retrieval_ms = result["timing"]["retrieval_micros"] / 1000
            if not math.isfinite(retrieval_ms) or not 0 <= retrieval_ms <= elapsed + 1:
                raise ValueError(
                    "Server elapsed retrieval time exceeds client observation"
                )
            row_key = (qid, method)
            if row_key not in rows:
                m = metrics(ranked, query)
                supports = {d for d, grade in query["judgments"].items() if grade > 0}
                m["all_labeled_supports_at_10"] = bool(supports) and supports <= set(
                    ranked
                )
                if method.startswith("graph_"):
                    coverage = page["coverage"]
                    work = {
                        k: page[k]
                        for k in [
                            "seed",
                            "graph_coverage",
                            "affinity_work",
                            "graph_candidates",
                            "candidates_ranked",
                        ]
                    }
                else:
                    coverage = {
                        "source_complete": page["exhaustive"],
                        "candidates_complete": not page.get(
                            "candidates_truncated", False
                        ),
                        "graph_complete": True,
                        "embeddings_complete": True,
                    }
                    work = {k: v for k, v in page.items() if k not in ["hits"]}
                rows[row_key] = {
                    "query_id": qid,
                    "category": query["category"],
                    "method": method,
                    "ranked": ranked,
                    "hits": page["hits"],
                    "relations": page.get("relations", []),
                    "metrics": m,
                    "coverage": coverage,
                    "work": work,
                    "samples": [],
                }
            elif (
                rows[row_key]["ranked"] != ranked
                or rows[row_key]["hits"] != page["hits"]
                or rows[row_key]["relations"] != page.get("relations", [])
            ):
                raise ValueError("Frozen retrieval changed across repetitions")
            embedding_ms = (
                0
                if method in ["lexical", "graph_lexical_balanced"]
                else (None if generation.get("query_timing_status") == "unavailable_batch_capture"
                      else generation["queries"][qid]["elapsed_ms"])
            )
            rows[row_key]["samples"].append(
                {
                    "repetition": rep,
                    "retrieval_ms": retrieval_ms,
                    "http_ms": elapsed,
                    "embedding_plus_http_ms": None if embedding_ms is None else embedding_ms + elapsed,
                    "external_query_embedding_ms": embedding_ms,
                    **({"embedding_timing_status": "unavailable_batch_capture"} if embedding_ms is None else {}),
                    "response_bytes": byte_count,
                    "payload_bytes": sum(
                        len(canonical(h["record"]["payload"])) for h in page["hits"]
                    ),
                }
            )
            if (index + 1) % 50 == 0:
                report["rows"] = list(rows.values())
                save(report_path, report)
                print(
                    json.dumps({"completed": index + 1, "planned": len(jobs)}),
                    flush=True,
                )
        report["resources"] = monitor.finish()
        verify_sources(client, fixture, state, tag)
        verify_relations(client, graph, state)
        if fences(client, state["scope"]) != before:
            raise ValueError("Memory or relation history changed during evaluation")
        verify_seed_baselines(rows, queries, protocol)
        report["rows"] = list(rows.values())
        report["summary"] = {
            m: summary([r for (q, method), r in rows.items() if method == m])
            for m in protocol["methods"]
        }
        report["categories"] = {
            c: {
                m: summary(
                    [
                        r
                        for (q, method), r in rows.items()
                        if method == m and r["category"] == c
                    ]
                )
                for m in protocol["methods"]
            }
            for c in sorted({q["category"] for q in queries.values()})
        }
        comparisons = {}
        primary = protocol["primary_comparison"]
        for baseline in ["lexical", "semantic", "weighted_rrf_v1", "weighted_rrf_v2"]:
            differences = {}
            for metric in [
                "ndcg_at_10",
                "judged_recall_at_10",
                "all_labeled_supports_at_10",
            ]:
                deltas = [
                    float(rows[(q, primary["candidate"])]["metrics"][metric])
                    - float(rows[(q, baseline)]["metrics"][metric])
                    for q in sorted(queries)
                ]
                differences[metric] = {
                    "mean_delta": statistics.mean(deltas),
                    "paired_interval": paired_interval(deltas, protocol["seed"]),
                    "wins": sum(d > 1e-12 for d in deltas),
                    "losses": sum(d < -1e-12 for d in deltas),
                    "ties": sum(abs(d) <= 1e-12 for d in deltas),
                }
            comparisons[baseline] = {
                "primary_predeclared": baseline == primary["baseline"],
                "metrics": differences,
            }
        if "support_comparisons" in protocol:
            from graph_report_evidence import support_transitions
            report["support_transitions"] = support_transitions(report["rows"], protocol, queries)
        if "seed_comparisons" in protocol:
            from graph_report_evidence import seed_comparisons
            report["seed_comparisons"] = seed_comparisons(report["rows"], protocol, queries)
        report.update(
            status="completed",
            comparisons=comparisons,
            finished_at=datetime.now(timezone.utc).isoformat(),
            verified_fences_unchanged=True,
            verified_current_sources_and_relations=True,
        )
    except Exception as exc:
        report.update(
            status="failed", rows=list(rows.values()), resources=monitor.finish()
        )
        report["failures"].append({"type": type(exc).__name__, "message": str(exc)})
        save(report_path, report)
        raise
    save(report_path, report)
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=["prepare", "evaluate"])
    for name in ["fixture", "relations", "credential-file", "state", "protocol"]:
        p.add_argument("--" + name, type=Path, required=True)
    for name in ["plan", "report", "generation"]:
        p.add_argument("--" + name, type=Path)
    p.add_argument("--server-pid", type=int)
    a = p.parse_args()
    fixture = json.loads(a.fixture.read_text())
    graph = json.loads(a.relations.read_text())
    protocol = json.loads(a.protocol.read_text())
    verify_protocol(protocol, fixture, graph)
    access = json.loads(a.credential_file.read_text())
    client = Client(access["api_url"], access["secret"])
    with state_lock(a.state):
        if a.action == "prepare":
            started = time.perf_counter()
            state, _ = prepare(client, fixture, access["scope"], a.state)
            prepare_relations(client, fixture, graph, state, a.state)
            client.call(
                "POST",
                "/api/v2/memory/changes/activate",
                {"contract_version": 2, "scope": state["scope"]},
            )
            verify_relations(client, graph, state)
            save(
                a.state.with_suffix(".preparation.json"),
                {
                    "elapsed_ms": (time.perf_counter() - started) * 1000,
                    "documents": len(state["documents"]),
                    "relations": len(state["graph"]["relations"]),
                    "fixture_sha256": digest(fixture),
                    "manifest_sha256": digest(state),
                },
            )
            print(
                json.dumps(
                    {
                        "prepared_documents": len(state["documents"]),
                        "prepared_relations": len(state["graph"]["relations"]),
                    }
                )
            )
        else:
            if not all([a.plan, a.report, a.generation]):
                p.error("Evaluation requires --plan, --report and --generation")
            report = evaluate(
                client,
                fixture,
                graph,
                json.loads(a.state.read_text()),
                protocol,
                a.plan,
                a.report,
                json.loads(a.generation.read_text()),
                a.server_pid,
            )
            print(
                json.dumps({"status": report["status"], "summary": report["summary"]}),
                flush=True,
            )


if __name__ == "__main__":
    main()
