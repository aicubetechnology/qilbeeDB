#!/usr/bin/env python3
"""Freeze an externally judged multi-hop corpus and document-only typed relations.

Never use questions, answers, decompositions or supporting labels to build edges.
The reserved cohort is an identifier-selected slice of the public development set,
not MuSiQue's hidden test leaderboard. Keep generated corpora/vectors outside Git.
"""

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import unicodedata

from build_embedding_fixture import validate_source
from evaluate_retrieval import digest, save

SOURCE_HASHES = {
    "musique_ans_v1.0_train.jsonl": "83a75b1e11e4e9bb8f8308e72ac40ca617ae4431b3a0d955b61cab259248490a",
    "musique_ans_v1.0_dev.jsonl": "15fa63794d18a94ce12411aca6e2327e65b6e83b0b1490efab3f1962e48abf3b",
}
ARCHIVE_SHA256 = "98f839bf2fd5319f5c688aed77901a6d5c30b3b9f9f691ab9a8ecafb045ee0cd"
SELECTION_VERSION = "musique_identifier_strata_v1"
SELECTION_SEED = "qilbeedb-graph-20260921-v1"
COUNTS = {"development": {2: 10, 3: 10, 4: 10}, "test": {2: 40, 3: 30, 4: 30}}
GRAPH_POLICY = {
    "version": "document_title_graph_v1",
    "normalization": "Unicode NFKC then casefold, Unicode word tokens joined by one space",
    "minimum_single_token_title_characters": 4,
    "same_title_neighbors_per_document": 4,
    "mention_targets_per_document": 8,
    "mention_order": "normalized title character length descending, target document ID ascending",
    "same_title_order": "target document ID ascending; one canonical directed assertion per pair",
    "mention_text": "paragraph text only; full normalized titles; no aliases or disambiguation stripping",
    "inputs": ["id", "title", "paragraph_text"],
    "label_access": "none: no query, answer, decomposition or relevance judgment accepted",
}


def words(text):
    return re.findall(r"\w+", unicodedata.normalize("NFKC", text).casefold())


def identity(title, text):
    return "musique-" + digest({"title": title, "paragraph_text": text})


def document_graph(documents):
    """Accept only a document projection, so label-dependent edges cannot hide here."""
    if any(set(d) != {"id", "title", "paragraph_text"} for d in documents):
        raise ValueError("Graph construction accepts document fields only")
    if len({d["id"] for d in documents}) != len(documents):
        raise ValueError("Duplicate graph document ID")
    titles = defaultdict(list)
    for doc in documents:
        if doc["id"] != identity(doc["title"], doc["paragraph_text"]):
            raise ValueError("Graph document identity differs from its text")
        titles[" ".join(words(doc["title"]))].append(doc["id"])
    for ids in titles.values():
        ids.sort()
    eligible = {
        title: ids
        for title, ids in titles.items()
        if title
        and not title.isdecimal()
        and (
            " " in title
            or len(title) >= GRAPH_POLICY["minimum_single_token_title_characters"]
        )
    }
    counts = Counter(token for title in eligible for token in set(title.split()))
    inverted = defaultdict(list)
    for title in sorted(eligible):
        token = min(set(title.split()), key=lambda t: (counts[t], t))
        inverted[token].append(title)
    edges = {}

    def edge(source, target, kind, evidence):
        key = (source, target, kind)
        edges[key] = {
            "id": "edge-" + digest(key),
            "source": source,
            "target": target,
            "kind": kind,
            "provenance": {
                "origin": "tool_observation",
                "method": GRAPH_POLICY["version"],
                "method_revision": digest(GRAPH_POLICY),
                "evidence_ref": evidence,
                "model": None,
            },
        }

    for doc in sorted(documents, key=lambda d: d["id"]):
        own_title = " ".join(words(doc["title"]))
        same = [x for x in titles[own_title] if x != doc["id"]]
        for other in same[: GRAPH_POLICY["same_title_neighbors_per_document"]]:
            source, target = sorted([doc["id"], other])
            edge(source, target, "same_entity", "title-sha256:" + digest(own_title))
        tokens = words(doc["paragraph_text"])
        text = " " + " ".join(tokens) + " "
        candidates = set()
        for token in set(tokens):
            for title in inverted.get(token, []):
                if title != own_title and " " + title + " " in text:
                    candidates.update((title, target) for target in eligible[title])
        selected = sorted(candidates, key=lambda v: (-len(v[0]), v[1]))[
            : GRAPH_POLICY["mention_targets_per_document"]
        ]
        for title, target in selected:
            edge(
                doc["id"],
                target,
                "semantic_related",
                "document-sha256:" + digest(doc) + ":title-sha256:" + digest(title),
            )
    result = sorted(edges.values(), key=lambda e: e["id"])
    return result, {
        "policy": GRAPH_POLICY,
        "policy_sha256": digest(GRAPH_POLICY),
        "document_projection_sha256": digest(sorted(documents, key=lambda d: d["id"])),
        "relations_sha256": digest(result),
        "documents": len(documents),
        "relations": len(result),
        "relation_kinds": dict(Counter(e["kind"] for e in result)),
        "normalized_titles": len(titles),
        "eligible_mention_titles": len(eligible),
    }


def select(rows, split, excluded_text=()):
    groups = defaultdict(list)
    excluded = set(excluded_text)
    for row in rows:
        if row["answerable"] is not True:
            raise ValueError("This protocol requires MuSiQue-Ans")
        if row["question"].strip().casefold() in excluded:
            continue
        hop = int(row["id"][0])
        if hop not in (2, 3, 4):
            raise ValueError("Unknown hop stratum")
        groups[hop].append(row)
    chosen = []
    for hop, count in COUNTS[split].items():
        ordered = sorted(
            groups[hop], key=lambda r: (digest([SELECTION_SEED, r["id"]]), r["id"])
        )
        if len(ordered) < count:
            raise ValueError("Insufficient rows for frozen stratum")
        chosen.extend(ordered[:count])
    if len({r["question"].strip().casefold() for r in chosen}) != len(chosen):
        raise ValueError(
            "Repeated query text in selected cohort; revise the protocol explicitly"
        )
    return sorted(chosen, key=lambda r: r["id"])


def build(train, reserved, hashes):
    selected_test = select(reserved, "test")
    selected_development = select(
        train, "development", {r["question"].strip().casefold() for r in selected_test}
    )
    documents = {}
    queries = []
    selection = {}
    for split, rows in [("development", selected_development), ("test", selected_test)]:
        selection[split] = [r["id"] for r in rows]
        for row in rows:
            judgments = {}
            for paragraph in row["paragraphs"]:
                title, text = paragraph["title"], paragraph["paragraph_text"]
                did = identity(title, text)
                documents[did] = {"id": did, "title": title, "paragraph_text": text}
                if paragraph["is_supporting"]:
                    judgments[did] = 1
            if not judgments:
                raise ValueError("Selected query has no supporting judgments")
            queries.append(
                {
                    "id": row["id"],
                    "text": row["question"],
                    "split": split,
                    "category": row["id"].split("__")[0],
                    "judgments": judgments,
                    "judgments_complete": False,
                }
            )
    projection = sorted(documents.values(), key=lambda d: d["id"])
    relations, graph = document_graph(projection)
    source = {
        "schema_version": 1,
        "kind": "authorized_relevance",
        "provenance": {
            "dataset": "MuSiQue-Ans v1.0",
            "source": "https://github.com/StonyBrookNLP/musique",
            "paper": "https://arxiv.org/abs/2108.00573",
            "license": "CC BY 4.0",
            "attribution": "Harsh Trivedi, Niranjan Balasubramanian, Tushar Khot and Ashish Sabharwal; TACL 2022",
            "source_files_sha256": hashes,
            "archive_sha256": ARCHIVE_SHA256,
            "selection_version": SELECTION_VERSION,
            "selection_seed": SELECTION_SEED,
            "selected_ids": selection,
            "split_mapping": {
                "development": "official train subset",
                "test": "reserved official development subset; not hidden test",
            },
            "corpus": "deduplicated union of all selected questions' supplied paragraphs, including distractors",
            "judgments": "grade 1 for original supporting paragraphs; other union-corpus documents unjudged",
        },
        "documents": [
            {"id": d["id"], "text": d["title"] + "\n" + d["paragraph_text"]}
            for d in projection
        ],
        "queries": queries,
    }
    validate_source(source)
    graph.update(
        schema_version=1,
        source_sha256=digest(source),
        projection=projection,
        relations=relations,
    )
    return source, graph


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--relations", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.relations.exists():
        parser.error("Preserve frozen outputs; choose new paths")
    sources = []
    for name, expected in SOURCE_HASHES.items():
        raw = (args.source_dir / name).read_bytes()
        if hashlib.sha256(raw).hexdigest() != expected:
            raise ValueError("Source digest mismatch: " + name)
        sources.append([json.loads(line) for line in raw.splitlines()])
    source, graph = build(*sources, SOURCE_HASHES)
    save(args.output, source)
    save(args.relations, graph)
    print(
        json.dumps(
            {
                "source_sha256": digest(source),
                "documents": len(source["documents"]),
                "queries": len(source["queries"]),
                "relations": len(graph["relations"]),
                "relation_kinds": graph["relation_kinds"],
            }
        )
    )


if __name__ == "__main__":
    main()
