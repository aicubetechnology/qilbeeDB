"""Pin the path-strength development experiment to an exact versioned source."""
import copy
import json
from pathlib import Path

from evaluate_retrieval import canonical, digest

PROTOCOL = Path(__file__).resolve().parents[1] / "benchmarks/retrieval/graph-path-strength-development-v1.json"


def verify_path_strength_protocol(protocol, fixture):
    # This checked-in file belongs to the frozen evaluator revision.
    expected = json.loads(PROTOCOL.read_text())
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
        q["id"] for q in queries if q["split"] == "development"
    ) != expected["measured_query_ids"]:
        raise ValueError("Path-strength measured query cohort differs")
