#!/usr/bin/env python3
"""Materialize a replayed cohort with accurate split provenance and bundle checks."""
import argparse
import hashlib
import json
import os
from pathlib import Path

from evaluate_retrieval import digest
from import_musique_graph import build_selected
import select_disjoint_musique as selection


def materialize(manifest, source_bytes, prior_bytes, warmup_bytes):
    selection.verify(manifest, source_bytes, prior_bytes)
    warmup_sha = hashlib.sha256(warmup_bytes).hexdigest()
    if warmup_sha not in manifest["prior_fixture_sha256"]:
        raise ValueError("Warm-up fixture was not part of the frozen exclusion history")
    warmup = json.loads(warmup_bytes)
    warmup_ids = [q["id"] for q in warmup["queries"] if q["split"] == "development"]
    if not warmup_ids or len(warmup_ids) != len(set(warmup_ids)):
        raise ValueError("Warm-up requires unique, previously observed development queries")
    train = {r["id"]: r for r in map(json.loads, source_bytes["musique_ans_v1.0_train.jsonl"].splitlines())}
    if not set(warmup_ids) <= set(train):
        raise ValueError("Warm-up queries must belong to the declared official training source")
    pool = {r["id"]: r for r in map(json.loads, source_bytes[f"musique_ans_v1.0_{manifest['source_split']}.jsonl"].splitlines())}
    selected_ids = sorted(qid for ids in manifest["selected_ids"].values() for qid in ids)
    if set(selected_ids) & set(warmup_ids):
        raise ValueError("Measured queries cannot also be warm-up queries")
    source, graph = build_selected([train[qid] for qid in sorted(warmup_ids)],
                                   [pool[qid] for qid in selected_ids], dict(selection.SOURCE_HASHES))
    source["provenance"].update(
        selection_version=manifest["selection_version"], selection_seed=manifest["seed"],
        selection_manifest_sha256=digest(manifest),
        excluded_fixture_file_sha256=manifest["prior_fixture_sha256"],
        warmup_fixture_file_sha256=warmup_sha,
        selection_verification="Complete deterministic selection replay against pinned official sources and all declared historical fixtures",
        split_mapping={"development": "previously observed official training questions; warm-up only in the reserved comparison",
                       "test": f"new-to-this-evaluation public {manifest['source_split']} subset; not official hidden test or guaranteed model-unseen data"},
        selection_annotation_use=manifest["annotation_use"],
        selection_limits=manifest["limits"],
    )
    graph["source_sha256"] = digest(source)
    return source, graph


def receipt(source, graph):
    if graph["source_sha256"] != digest(source):
        raise ValueError("Graph belongs to another source revision")
    return {"schema_version": 1, "status": "materialized_not_evaluated",
            "source_sha256": digest(source), "graph_sha256": digest(graph),
            "selection_manifest_sha256": source["provenance"]["selection_manifest_sha256"],
            "warmup_fixture_file_sha256": source["provenance"]["warmup_fixture_file_sha256"],
            "documents": len(source["documents"]), "relations": len(graph["relations"]),
            "queries_by_split": {split: sum(q["split"] == split for q in source["queries"]) for split in ("development", "test")},
            "split_mapping": source["provenance"]["split_mapping"],
            "graph_inputs": "Document ID, title and paragraph text only; no question, answer, decomposition or supporting label",
            "embedding_generation_performed": False, "retrieval_performed": False}


def write_bundle(directory, source, graph):
    # Exclusive directory ownership; an interrupted bundle has no valid receipt.
    values = (("source.json", source), ("relations.json", graph), ("receipt.json", receipt(source, graph)))
    directory.mkdir()
    for name, value in values:
        with (directory / name).open("x") as stream:
            json.dump(value, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())


def verify_bundle(directory, source, graph):
    if directory.is_symlink():
        raise ValueError("Corpus bundle directory must not be a symlink")
    expected = {"source.json": source, "relations.json": graph, "receipt.json": receipt(source, graph)}
    for name, value in expected.items():
        path = directory / name
        if path.is_symlink() or not path.is_file() or json.loads(path.read_text()) != value:
            raise ValueError("Incomplete or altered corpus bundle: " + name)
    return expected["receipt.json"]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=("materialize", "verify"))
    p.add_argument("--source-dir", type=Path, required=True)
    p.add_argument("--prior-fixture", type=Path, action="append", required=True)
    p.add_argument("--warmup-fixture", type=Path, required=True)
    p.add_argument("--manifest", type=Path, required=True)
    p.add_argument("--bundle", type=Path, required=True)
    a = p.parse_args()
    if a.action == "materialize" and (a.bundle.exists() or a.bundle.is_symlink()):
        p.error("Preserve existing bundles; choose a new directory")
    source, graph = materialize(json.loads(a.manifest.read_text()),
        {name: (a.source_dir / name).read_bytes() for name in selection.SOURCE_HASHES},
        [path.read_bytes() for path in a.prior_fixture], a.warmup_fixture.read_bytes())
    if a.action == "materialize":
        write_bundle(a.bundle, source, graph)
    print(json.dumps(verify_bundle(a.bundle, source, graph)))


if __name__ == "__main__":
    main()
