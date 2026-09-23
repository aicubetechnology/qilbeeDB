"""Bind the reserved comparison to its versioned protocol and corpus provenance."""
import json
from pathlib import Path

PROTOCOL = Path(__file__).resolve().parents[1] / "benchmarks/retrieval/graph-balanced-v1-support-reserved-v1.json"


def verify_support_reserved(protocol, fixture, graph=None):
    # The file is part of the frozen tool revision, not a request-supplied policy.
    expected = json.loads(PROTOCOL.read_text())
    if protocol != expected:
        raise ValueError("Reserved comparison differs from its frozen versioned protocol")
    queries = fixture["queries"]
    ids = [q["id"] for q in queries]
    if len(ids) != len(set(ids)):
        raise ValueError("Duplicate reserved or warm-up query")
    if (sorted(q["id"] for q in queries if q["split"] == "test") != expected["measured_query_ids"]
        or sorted(q["id"] for q in queries if q["split"] == "development") != expected["warmup_query_ids"]
        or any(q["split"] not in ("test", "development") for q in queries)
        or len(fixture["documents"]) != expected["documents"]
        or (graph is not None and len(graph["relations"]) != expected["relations"])
        or fixture["provenance"].get("selection_manifest_sha256") != expected["selection_manifest_sha256"]
        or fixture["provenance"].get("split_mapping") != expected["split_mapping"]):
        raise ValueError("Reserved queries, warm-up, counts or source split provenance differ")
