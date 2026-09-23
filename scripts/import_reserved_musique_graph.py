#!/usr/bin/env python3
"""Materialize an already frozen, component-disjoint public MuSiQue cohort.

The explicit selected-ID manifest is authoritative; this importer never selects
or replaces questions based on observed scores. Public development is not hidden
or guaranteed absent from model training. No model is called by this importer.
"""
import argparse
import hashlib
import json
from pathlib import Path

from evaluate_retrieval import digest, save
from import_musique_graph import SOURCE_HASHES, build_selected


def components(identifier):
    _, separator, suffix = identifier.partition("__")
    values = suffix.split("_")
    if not separator or not all(value.isdigit() for value in values):
        raise ValueError("Malformed MuSiQue component identity")
    return set(values)


def selected_rows(manifest, prior, rows):
    if manifest.get("selection_version") != "identifier_hash_disjoint_components_v1":
        raise ValueError("Unsupported frozen selection identity")
    if manifest.get("labels_used_for_selection") is not False:
        raise ValueError("Selection must declare that labels were not used")
    prior_ids = {q["id"] for q in prior["queries"]}
    excluded = set().union(*(components(qid) for qid in prior_ids))
    texts = {q["text"].strip().casefold() for q in prior["queries"]}
    index = {row["id"]: row for row in rows}
    if len(index) != len(rows):
        raise ValueError("Source contains duplicate query IDs")
    quotas = manifest["quotas"]
    if set(quotas) != {"2", "3", "4"} or set(manifest["selected_ids"]) != set(quotas):
        raise ValueError("Selection strata differ from declared quotas")
    chosen = []
    seen_ids = set()
    for hop, count in quotas.items():
        ids = manifest["selected_ids"][hop]
        if type(count) is not int or count < 1 or len(ids) != count:
            raise ValueError("Selection count differs from frozen quota")
        for qid in ids:
            if qid in seen_ids or qid in prior_ids or qid not in index or qid[0] != hop:
                raise ValueError("Duplicate, previously used, missing or wrong-stratum query")
            if components(qid) & excluded:
                raise ValueError("Reserved query shares a previously used component")
            row = index[qid]
            text = row["question"].strip().casefold()
            if not text or text in texts or row["answerable"] is not True:
                raise ValueError("Duplicate, empty or unanswerable reserved query")
            seen_ids.add(qid)
            texts.add(text)
            chosen.append(row)
    return sorted(chosen, key=lambda row: row["id"])


def materialize(manifest, prior_bytes, train_bytes, reserved_bytes):
    if hashlib.sha256(prior_bytes).hexdigest() != manifest["excluded_fixture_sha256"]:
        raise ValueError("Previously used fixture digest differs")
    for name, raw in [("musique_ans_v1.0_train.jsonl", train_bytes),
                      ("musique_ans_v1.0_dev.jsonl", reserved_bytes)]:
        if hashlib.sha256(raw).hexdigest() != SOURCE_HASHES[name]:
            raise ValueError("Official source digest differs")
    if manifest["source_file_sha256"] != SOURCE_HASHES["musique_ans_v1.0_dev.jsonl"]:
        raise ValueError("Manifest source digest differs")
    prior = json.loads(prior_bytes)
    train = [json.loads(line) for line in train_bytes.splitlines()]
    reserved = [json.loads(line) for line in reserved_bytes.splitlines()]
    selected = selected_rows(manifest, prior, reserved)
    development_ids = {q["id"] for q in prior["queries"] if q["split"] == "development"}
    development = sorted([r for r in train if r["id"] in development_ids], key=lambda r: r["id"])
    if len(development) != len(development_ids) or not development:
        raise ValueError("Previously frozen development cohort is incomplete")
    source, graph = build_selected(development, selected, SOURCE_HASHES)
    source["provenance"].update(
        selection_version="explicit_component_disjoint_manifest_v1",
        selection_seed=manifest["seed"],
        selection_manifest_sha256=digest(manifest),
        excluded_fixture_file_sha256=manifest["excluded_fixture_sha256"],
        selection_verification="Exact frozen IDs, source digests, prior IDs/components and normalized text disjointness; original hash-selection procedure is not replayed.",
    )
    graph["source_sha256"] = digest(source)
    return source, graph


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["manifest", "prior-source", "source-dir", "output", "relations"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.relations.exists():
        parser.error("Preserve frozen outputs; choose new paths")
    source, graph = materialize(
        json.loads(args.manifest.read_text()), args.prior_source.read_bytes(),
        (args.source_dir / "musique_ans_v1.0_train.jsonl").read_bytes(),
        (args.source_dir / "musique_ans_v1.0_dev.jsonl").read_bytes(),
    )
    save(args.output, source)
    save(args.relations, graph)
    print(json.dumps({"documents": len(source["documents"]), "queries": len(source["queries"]),
                      "relations": len(graph["relations"]), "source_sha256": digest(source)}))


if __name__ == "__main__":
    main()
