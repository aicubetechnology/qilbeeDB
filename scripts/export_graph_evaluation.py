#!/usr/bin/env python3
"""Export auditable rankings and costs without source text, vectors or credentials."""

import argparse
import json
from pathlib import Path

from evaluate_retrieval import digest, metrics, save
from evaluate_graph_retrieval import summary, evaluation_stage, evaluation_queries
from graph_report_evidence import categories, comparisons, seed_comparisons
from graph_measurement_evidence import validate_measurements


def export(report, fixture):
    plan = report["plan"]
    if (
        report["status"] != "completed"
        or report["failures"]
        or not report["verified_fences_unchanged"]
        or not report["verified_current_sources_and_relations"]
        or plan["fixture_sha256"] != digest(fixture)
    ):
        raise ValueError("Only completed, source-verified evidence can be exported")
    protocol = plan["protocol"]
    if plan["protocol_sha256"] != digest(protocol):
        raise ValueError("Export protocol differs from the pinned plan")
    stage = evaluation_stage(protocol)
    if protocol["protocol_version"] == "graph_balanced_v1_support_reserved_v1":
        from support_reserved_contract import verify_support_reserved
        verify_support_reserved(protocol, fixture)
    if report.get("evaluation_stage", "reserved_comparison") != stage:
        raise ValueError("Report mislabels its evaluation stage")
    queries = evaluation_queries(fixture, protocol)
    expected = {(qid, method) for qid in queries for method in protocol["methods"]}
    seen = set()
    rows = []
    for row in report["rows"]:
        key = (row["query_id"], row["method"])
        if key not in expected or key in seen:
            raise ValueError(
                "Unexpected, duplicate or missing query/method observation"
            )
        seen.add(key)
        query = queries[row["query_id"]]
        if row["category"] != query["category"]:
            raise ValueError("Query category differs from the frozen fixture")
        recalculated = metrics(row["ranked"], query)
        supports = {did for did, grade in query["judgments"].items() if grade > 0}
        recalculated["all_labeled_supports_at_10"] = bool(supports) and supports <= set(
            row["ranked"]
        )
        if recalculated != row["metrics"]:
            raise ValueError(
                "Published metrics differ from recorded judgments and ranking"
            )
        validate_measurements(row, protocol["repetitions"])
        rows.append(
            {
                k: row[k]
                for k in [
                    "query_id",
                    "category",
                    "method",
                    "ranked",
                    "metrics",
                    "coverage",
                    "samples",
                ]
            }
            | {
                "work": {k: v for k, v in row["work"].items() if k != "seed"},
                "seed_work": {
                    k: v
                    for k, v in row["work"].get("seed", {}).items()
                    if k
                    not in ["selected_anchors", "embedding_space", "hybrid_profile"]
                },
                "proofs_sha256": digest(
                    {"hits": row["hits"], "relations": row["relations"]}
                ),
            }
        )
    if seen != expected:
        raise ValueError("Incomplete query/method matrix")
    if {
        m: summary([r for r in rows if r["method"] == m]) for m in protocol["methods"]
    } != report["summary"]:
        raise ValueError("Summary does not match the measured rows")
    if categories(rows, protocol, queries) != report["categories"]:
        raise ValueError("Category aggregates differ from verified query rows")
    if comparisons(rows, protocol, queries) != report["comparisons"]:
        raise ValueError("Paired comparisons differ from verified query rows")
    extra = {}
    if "seed_comparisons" in protocol:
        expected_seed_comparisons = seed_comparisons(rows, protocol, queries)
        if report.get("seed_comparisons") != expected_seed_comparisons:
            raise ValueError("Paired seed comparisons differ from verified query rows")
        extra["seed_comparisons"] = expected_seed_comparisons
    elif "seed_comparisons" in report:
        raise ValueError("Seed comparisons were not predeclared")
    resources = report["resources"]
    return {
        **extra,
        "schema_version": 1,
        "status": "completed",
        "evaluation_stage": stage,
        "default_admission": False,
        "scope_of_evidence": report["scope_of_evidence"],
        "started_at": report["started_at"],
        "finished_at": report["finished_at"],
        "raw_report_sha256": digest(report),
        "plan": {
            k: plan[k]
            for k in [
                "protocol_sha256",
                "fixture_sha256",
                "graph_sha256",
                "manifest_sha256",
                "generation_sha256",
                "server_health",
                "execution_limits",
                "model_space",
                "protocol",
            ]
        },
        "environment": report["environment"],
        "source_provenance": fixture["provenance"],
        "embedding_provenance": fixture["embedding_provenance"],
        "documents": len(fixture["documents"]),
        "queries": [
            {k: q[k] for k in ["id", "category", "judgments", "judgments_complete"]}
            for q in sorted(queries.values(), key=lambda q: q["id"])
        ],
        "summary": report["summary"],
        "categories": report["categories"],
        "comparisons": report["comparisons"],
        "failures": [],
        "verified_fences_unchanged": True,
        "verified_current_sources_and_relations": True,
        "resources": {
            k: resources[k]
            for k in [
                "available",
                "errors",
                "peak_sampled_rss_bytes",
                "cpu_seconds_delta",
                "scope",
            ]
        },
        "rows": sorted(rows, key=lambda r: (r["query_id"], r["method"])),
        "retained_private_evidence": "Full current source payloads, embedding receipts, typed path proofs, scope bindings, process samples and frozen vectors are bound by the recorded hashes; excluded from this public export.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["report", "fixture", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    result = export(
        json.loads(args.report.read_text()), json.loads(args.fixture.read_text())
    )
    save(args.output, result)
    print(json.dumps({"export_sha256": digest(result), "rows": len(result["rows"])}))


if __name__ == "__main__":
    main()
