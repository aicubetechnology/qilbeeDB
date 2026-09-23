#!/usr/bin/env python3
"""Select or replay a corpus cohort without retrieval results or model calls.

Support/answer annotations are used only to prevent overlap, never to construct
retrieval graphs. A public training subset is not an official held-out test set.
"""
import argparse
from collections import Counter
import hashlib
import json
import unicodedata
from pathlib import Path

from evaluate_retrieval import digest
from import_musique_graph import SOURCE_HASHES, words

VERSION = "musique_support_disjoint_greedy_v1"


def normalized(value):
    if not isinstance(value, str) or not value.strip():
        raise ValueError("Overlap identity must contain nonempty text")
    return " ".join(words(value)) or unicodedata.normalize("NFKC", value).casefold().strip()


def overlap_keys(row):
    """No ranking scores are accepted; labels only define leakage exclusions."""
    qid = row["id"]
    if not isinstance(qid, str) or qid[:1] not in "234" or "__" not in qid:
        raise ValueError("Malformed query identity")
    parts = qid.split("__", 1)[1].split("_")
    if len(parts) != int(qid[0]) or not all(p.isdecimal() for p in parts):
        raise ValueError("Malformed component identity")
    paragraphs = {p["idx"]: p for p in row["paragraphs"]}
    if len(paragraphs) != len(row["paragraphs"]):
        raise ValueError("Duplicate paragraph index")
    decomposition = row["question_decomposition"]
    if {str(d["id"]) for d in decomposition} != set(parts) or len(decomposition) != len(parts):
        raise ValueError("Decomposition differs from query identity")
    keys = {"query:" + normalized(row["question"]), "answer:" + normalized(row["answer"])}
    aliases = row.get("answer_aliases", [])
    if not isinstance(aliases, list):
        raise ValueError("Answer aliases must be a list")
    for alias in aliases:
        keys.add("answer:" + normalized(alias))
    for step in decomposition:
        paragraph = paragraphs.get(step["paragraph_support_idx"])
        if paragraph is None or paragraph["is_supporting"] is not True:
            raise ValueError("Decomposition support is missing or not supporting")
        keys.update(("component:" + str(step["id"]),
                     "subquestion:" + normalized(step["question"]),
                     "answer:" + normalized(step["answer"]),
                     "support:" + digest({"title": normalized(paragraph["title"]),
                                           "text": normalized(paragraph["paragraph_text"])})))
    return keys


def select_rows(rows, prior_rows, seed, quotas):
    if not isinstance(seed, str) or not seed.strip():
        raise ValueError("An explicit nonempty seed is required")
    if set(quotas) != set("234") or any(type(n) is not int or n < 1 for n in quotas.values()):
        raise ValueError("Positive integer quotas for two, three and four hops are required")
    if len({r["id"] for r in rows}) != len(rows):
        raise ValueError("Duplicate candidate query IDs")
    excluded = set()
    prior_ids = set()
    for row in prior_rows:
        prior_ids.add(row["id"])
        excluded.update(overlap_keys(row))
    eligible = []
    reasons = Counter()
    for row in rows:
        if row["answerable"] is not True:
            reasons["not_answerable"] += 1
            continue
        keys = overlap_keys(row)
        conflicts = keys & excluded
        if row["id"] in prior_ids:
            reasons["previous_id"] += 1
        elif conflicts:
            reasons.update(set(k.split(":", 1)[0] for k in conflicts))
        else:
            eligible.append((row, keys))
    # Higher-hop quotas are filled first, before lower-hop rows consume supports.
    used = set(excluded)
    selected = {hop: [] for hop in "234"}
    for hop in "432":
        candidates = sorted((item for item in eligible if item[0]["id"][0] == hop),
            key=lambda item: (hashlib.sha256((seed + "\n" + item[0]["id"]).encode()).hexdigest(), item[0]["id"]))
        for row, keys in candidates:
            if len(selected[hop]) == quotas[hop]:
                break
            if not keys & used:
                selected[hop].append(row["id"])
                used.update(keys)
        if len(selected[hop]) != quotas[hop]:
            raise ValueError(f"Insufficient disjoint {hop}-hop rows: requested {quotas[hop]}, selected {len(selected[hop])}; no quota was relaxed")
    return {"selected_ids": selected,
            "eligible_by_hop": {h: sum(r["id"][0] == h for r, _ in eligible) for h in "234"},
            "exclusion_counts": dict(sorted(reasons.items())),
            "previous_unique_queries": len(prior_ids), "selected_overlap_keys_sha256": digest(sorted(used - excluded))}


def build(source_bytes, prior_bytes, source_split, seed, quotas):
    if source_split not in ("train", "dev") or set(source_bytes) != set(SOURCE_HASHES):
        raise ValueError("Both pinned official sources and an explicit public split are required")
    sources = {}
    for name, raw in source_bytes.items():
        if hashlib.sha256(raw).hexdigest() != SOURCE_HASHES[name]:
            raise ValueError("Official source digest differs")
        sources[name] = [json.loads(line) for line in raw.splitlines()]
    index = {}
    for rows in sources.values():
        for row in rows:
            if row["id"] in index:
                raise ValueError("Duplicate official query identity")
            index[row["id"]] = row
    if not prior_bytes:
        raise ValueError("At least one previously used fixture is required")
    prior_ids = set()
    hashes = set()
    for raw in prior_bytes:
        hashes.add(hashlib.sha256(raw).hexdigest())
        fixture = json.loads(raw)
        if not fixture.get("queries"):
            raise ValueError("Empty prior fixture cannot establish exclusions")
        for query in fixture["queries"]:
            qid = query["id"]
            if qid not in index or normalized(query["text"]) != normalized(index[qid]["question"]):
                raise ValueError("Prior query does not match the pinned official source")
            prior_ids.add(qid)
    selected = select_rows(sources[f"musique_ans_v1.0_{source_split}.jsonl"],
                           [index[qid] for qid in sorted(prior_ids)], seed, quotas)
    return {"selection_version": VERSION, "source_file_sha256": dict(SOURCE_HASHES),
            "source_split": source_split, "prior_fixture_sha256": sorted(hashes),
            "seed": seed, "quotas": dict(quotas), "stratum_order": ["4", "3", "2"],
            "ordering": "SHA256(seed + newline + query ID), then query ID",
            "normalization": "NFKC casefold Unicode word tokens joined by one space; punctuation-only text retains its normalized literal",
            "annotation_use": "Answerability eligibility and support/answer overlap exclusions only; no retrieval scores or quality-based selection",
            "retrieval_results_used": False, **selected,
            "limits": ["Public data; not guaranteed absent from model training",
                       "Greedy strata and exclusions change the source distribution; not a uniform sample",
                       "Declared identity disjointness does not prove statistical independence",
                       "Unlisted historical fixtures cannot be excluded automatically",
                       "Shared distractors and semantically equivalent answers may remain"]}


def verify(manifest, source_bytes, prior_bytes):
    replayed = build(source_bytes, prior_bytes, manifest["source_split"], manifest["seed"], manifest["quotas"])
    if manifest != replayed:
        raise ValueError("Selection manifest does not reproduce exactly")
    return replayed


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=("select", "verify"))
    p.add_argument("--source-dir", type=Path, required=True)
    p.add_argument("--prior-fixture", type=Path, action="append", required=True)
    p.add_argument("--manifest", type=Path, required=True)
    p.add_argument("--source-split", choices=("train", "dev"))
    p.add_argument("--seed")
    p.add_argument("--quotas", type=int, nargs=3, metavar=("TWO", "THREE", "FOUR"))
    a = p.parse_args()
    sources = {name: (a.source_dir / name).read_bytes() for name in SOURCE_HASHES}
    previous = [path.read_bytes() for path in a.prior_fixture]
    if a.action == "select":
        if not a.source_split or not a.seed or not a.quotas:
            p.error("Selection requires split, seed and all three quotas")
        result = build(sources, previous, a.source_split, a.seed, dict(zip("234", a.quotas)))
        with a.manifest.open("x") as f:
            json.dump(result, f, indent=2)
            f.write("\n")
    else:
        if a.source_split or a.seed or a.quotas:
            p.error("Verification reads selection settings from the manifest")
        result = verify(json.loads(a.manifest.read_text()), sources, previous)
    print(json.dumps({"manifest_sha256": digest(result), "selected": sum(result["quotas"].values()),
                      "source_split": result["source_split"], "eligible_by_hop": result["eligible_by_hop"]}))


if __name__ == "__main__":
    main()
