"""Pin the path-strength development experiment to an exact versioned source."""
import copy
import hashlib
import json
from pathlib import Path

from evaluate_retrieval import canonical, digest

PROTOCOL = Path(__file__).resolve().parents[1] / "benchmarks/retrieval/graph-path-strength-development-v1.json"

REGRESSION_PROTOCOL = PROTOCOL.with_name("graph-path-strength-regression-v1.json")

def verify_path_strength_protocol(protocol, fixture):
    # This checked-in file belongs to the frozen evaluator revision.
    regression = protocol.get("protocol_version") == "graph_path_strength_regression_v1"
    path = REGRESSION_PROTOCOL if regression else PROTOCOL
    expected = json.loads(path.read_text())
    if canonical(protocol) != canonical(expected):
        raise ValueError("Path-strength comparison differs from its frozen protocol")
    source = copy.deepcopy(fixture)
    source.pop("space")
    source.pop("embedding_provenance")
    for group in ("documents", "queries"):
        for row in source[group]:
            row.pop("vector")
    if digest(source) != expected["source_sha256"]:
        raise ValueError("Path-strength source differs from the frozen corpus")
    queries = fixture["queries"]
    ids = [q["id"] for q in queries]
    if len(ids) != len(set(ids)) or sorted(
        q["id"] for q in queries if q["split"] == expected["split"]
    ) != expected["measured_query_ids"]:
        raise ValueError("Path-strength measured query cohort differs")
    if regression:
        prior = Path(__file__).resolve().parents[1] / expected["prior_observation"]["report"]
        if hashlib.sha256(prior.read_bytes()).hexdigest() != expected["prior_observation"]["sha256"]:
            raise ValueError("Prior observation artifact differs from the frozen history")
        if (sorted(q["id"] for q in queries if q["split"] == "development") != expected["warmup_query_ids"]
            or len(fixture["documents"]) != expected["documents"]
            or fixture["provenance"].get("selection_manifest_sha256") != expected["selection_manifest_sha256"]
            or fixture["provenance"].get("split_mapping") != expected["split_mapping"]):
            raise ValueError("Observed cohort provenance or warm-up differs")
