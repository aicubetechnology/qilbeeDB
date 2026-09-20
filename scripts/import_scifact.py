#!/usr/bin/env python3
"""Import the pinned BEIR SciFact archive as an externally judged text fixture."""

import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import zipfile

from build_embedding_fixture import validate_source
from evaluate_retrieval import save

ARCHIVE_SHA256 = "536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165"
SOURCE_URL = (
    "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/scifact.zip"
)


def convert(corpus, queries, train, test, provenance):
    documents = [
        {
            "id": row["_id"],
            "text": "\n".join(
                part for part in [row.get("title", ""), row["text"]] if part
            ),
        }
        for row in corpus
    ]
    known = {d["id"] for d in documents}
    by_id = {row["_id"]: row["text"] for row in queries}
    if len(known) != len(documents) or len(by_id) != len(queries):
        raise ValueError("Duplicate upstream document or query ID")
    selected = []
    assigned = set()
    for split, rows in [("development", train), ("test", test)]:
        judgments = {}
        for row in rows:
            qid, did = row["query-id"], row["corpus-id"]
            if qid not in by_id or did not in known or row["score"] != "1":
                raise ValueError("Invalid upstream qrel reference or grade")
            values = judgments.setdefault(qid, {})
            if did in values:
                raise ValueError("Duplicate upstream judgment")
            values[did] = 1
        if assigned & judgments.keys():
            raise ValueError("A query occurs in both upstream splits")
        assigned.update(judgments)
        for qid, values in judgments.items():
            selected.append(
                {
                    "id": qid,
                    "text": by_id[qid],
                    "split": split,
                    "category": "scientific_claim",
                    "judgments": values,
                    "judgments_complete": False,
                }
            )
    # Public split IDs are disjoint, but some claim texts repeat across splits.
    # Exclude only development duplicates; preserve every official test judgment.
    reserved_text = {
        q["text"].strip().casefold() for q in selected if q["split"] == "test"
    }
    excluded = [
        q["id"]
        for q in selected
        if q["split"] == "development" and q["text"].strip().casefold() in reserved_text
    ]
    selected = [q for q in selected if q["id"] not in excluded]
    provenance = dict(provenance, excluded_development_duplicate_ids=excluded)
    fixture = {
        "schema_version": 1,
        "kind": "authorized_relevance",
        "provenance": provenance,
        "documents": documents,
        "queries": selected,
    }
    validate_source(fixture)
    return fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output exists; preserve the frozen input and choose a new path")
    raw = args.archive.read_bytes()
    if hashlib.sha256(raw).hexdigest() != ARCHIVE_SHA256:
        raise ValueError(
            "SciFact archive digest differs from the pinned upstream release"
        )
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        names = [
            "scifact/corpus.jsonl",
            "scifact/queries.jsonl",
            "scifact/qrels/train.tsv",
            "scifact/qrels/test.tsv",
        ]
        files = {name: archive.read(name) for name in names}
    corpus, queries = [
        [json.loads(line) for line in files[name].decode().splitlines()]
        for name in names[:2]
    ]
    train, test = [
        list(csv.DictReader(io.StringIO(files[name].decode()), delimiter="\t"))
        for name in names[2:]
    ]
    provenance = {
        "dataset": "BEIR SciFact",
        "source_url": SOURCE_URL,
        "archive_sha256": ARCHIVE_SHA256,
        "file_sha256": {
            name: hashlib.sha256(value).hexdigest() for name, value in files.items()
        },
        "judgments": "Original BEIR binary qrels: grade 1 relevant, unlisted documents unjudged. Not exhaustive relevance coverage.",
        "split_mapping": "BEIR train -> development, excluding exact case-insensitive text duplicates of test claims; BEIR test -> test unchanged. No negative labels invented.",
        "text_mapping": "Nonempty title and abstract joined by one newline; full source text preserved.",
        "original_dataset": "https://github.com/allenai/scifact",
        "license_reference": "https://github.com/allenai/scifact/blob/master/LICENSE.md",
        "scope": "Scientific claim evidence retrieval; not clinical advice, truth verification or an agent-task benchmark.",
    }
    fixture = convert(corpus, queries, train, test, provenance)
    if (
        len(fixture["documents"]) != 5183
        or sum(q["split"] == "test" for q in fixture["queries"]) != 300
    ):
        raise ValueError("Unexpected upstream corpus or query count")
    save(args.output, fixture)
    print(
        "Imported 5,183 abstracts; development queries:",
        sum(q["split"] == "development" for q in fixture["queries"]),
        "; test queries: 300; excluded development duplicates:",
        len(fixture["provenance"]["excluded_development_duplicate_ids"]),
    )


if __name__ == "__main__":
    main()
