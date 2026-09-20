#!/usr/bin/env python3
"""Measure external CPU query generation plus HTTP retrieval against frozen vectors."""

import argparse
import json
from pathlib import Path
import random
import time

from build_embedding_fixture import E5Encoder
from evaluate_retrieval import (
    Client,
    PROFILES,
    digest,
    quantile,
    query_request,
    save,
    state_lock,
    validate_fixture,
    verify_sources,
)


def measure(client, fixture, state, encoder, repetitions, split, ranking_version):
    validate_fixture(fixture)
    if encoder.space != fixture["space"] or ranking_version not in PROFILES:
        raise ValueError(
            "Encoder identity or ranking version differs from the frozen trial"
        )
    tag = "retrieval-fixture-" + digest(fixture)
    verify_sources(client, fixture, state, tag)
    queries = [q for q in fixture["queries"] if split == "all" or q["split"] == split]
    rows = []
    rng = random.Random(20260920)
    for repetition in range(-1, repetitions):
        jobs = [
            (q, mode) for q in queries for mode in ["lexical", "semantic", "hybrid"]
        ]
        rng.shuffle(jobs)
        for query, mode in jobs:
            started = time.perf_counter()
            generation = None
            if mode != "lexical":
                vector, generation = encoder.encode(query["text"], "query")
                if vector != query["vector"]:
                    raise ValueError(
                        "Regenerated query vector differs; preserve the original trial and freeze a new fixture"
                    )
            path, body = query_request(
                mode, query, fixture, state["scope"], tag, ranking_version
            )
            result, http_ms, size = client.call("POST", path, body)
            elapsed = (time.perf_counter() - started) * 1000
            if (
                result["scope"] != state["scope"]
                or not result["page"]["exhaustive"]
                or result["page"]["next_after"] is not None
            ):
                raise ValueError(
                    "Client-cycle request has invalid scope or incomplete coverage"
                )
            if mode == "hybrid" and (
                result["page"]["ranking"] != PROFILES[ranking_version]
                or result["page"]["embedding_coverage"] != "complete"
            ):
                raise ValueError(
                    "Client-cycle hybrid profile or embedding coverage differs"
                )
            if repetition >= 0:
                rows.append(
                    {
                        "query_id": query["id"],
                        "mode": mode,
                        "generation_ms": generation["elapsed_ms"] if generation else 0,
                        "http_ms": http_ms,
                        "end_to_end_ms": elapsed,
                        "retrieval_ms": result["timing"]["retrieval_micros"] / 1000,
                        "response_bytes": size,
                    }
                )
    verify_sources(client, fixture, state, tag)
    return {
        "schema_version": 1,
        "fixture_sha256": digest(fixture),
        "manifest_sha256": digest(state),
        "ranking_version": ranking_version,
        "split": split,
        "queries": len(queries),
        "repetitions": repetitions,
        "conditions": "Serial client; one excluded warmup pass; seeded shuffled order. Full cycle includes tokenization/inference/pooling/normalization, request construction, HTTP and parsing; excludes model/session loading and source verification. Lexical does not generate vectors. Each generated vector must exactly equal its frozen value. No other experiment should run concurrently.",
        "summary": {
            mode: {
                field: {
                    key: quantile([r[field] for r in rows if r["mode"] == mode], p)
                    for key, p in [("p50", 0.5), ("p95", 0.95)]
                }
                for field in [
                    "generation_ms",
                    "http_ms",
                    "end_to_end_ms",
                    "retrieval_ms",
                ]
            }
            for mode in ["lexical", "semantic", "hybrid"]
        },
        "rows": rows,
        "total_cost": None,
        "cost_reason": "No paid provider calls; local hardware and energy costs unmeasured.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["fixture", "credential-file", "state", "model-dir", "report"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--base-url", default="http://127.0.0.1:7474")
    parser.add_argument(
        "--overlength", choices=["reject", "truncate"], default="reject"
    )
    parser.add_argument(
        "--split", choices=["all", "development", "test"], default="test"
    )
    parser.add_argument(
        "--ranking-version", choices=sorted(PROFILES), default="weighted_rrf_v1"
    )
    parser.add_argument("--repetitions", type=int, default=1)
    args = parser.parse_args()
    if not 1 <= args.repetitions <= 100:
        parser.error("repetitions must be in 1..100")
    fixture = json.loads(args.fixture.read_text())
    encoder = E5Encoder(args.model_dir, args.overlength)
    client = Client(
        args.base_url, json.loads(args.credential_file.read_text())["secret"]
    )
    with state_lock(args.state):
        state = json.loads(args.state.read_text())
        report = measure(
            client,
            fixture,
            state,
            encoder,
            args.repetitions,
            args.split,
            args.ranking_version,
        )
        save(args.report, report)
    print("Wrote full client-cycle measurements:", args.report)


if __name__ == "__main__":
    main()
