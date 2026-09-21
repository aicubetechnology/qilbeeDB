#!/usr/bin/env python3
"""Compare current GETs and batch reads on frozen, explicitly authorized records.

Read-only: no imports, writes, retries, provider calls or credential persistence.
The input JSON contains scope and 1-100 record_ids. Every observed record must
match the initial batch, including revisions and contents, or the run fails.
"""

import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import statistics
import time
import urllib.parse
import uuid


class Client:
    def __init__(self, base, token):
        parsed = urllib.parse.urlsplit(base)
        if (
            parsed.scheme not in ("http", "https")
            or not parsed.hostname
            or parsed.username
            or parsed.password
            or parsed.query
            or parsed.fragment
            or parsed.path not in ("", "/")
        ):
            raise ValueError("Expected an HTTP(S) origin without credentials or a path")
        kind = (
            http.client.HTTPSConnection
            if parsed.scheme == "https"
            else http.client.HTTPConnection
        )
        self.connection = kind(parsed.hostname, parsed.port, timeout=30)
        self.token = token

    def call(self, method, route, body=None):
        payload = None if body is None else json.dumps(body).encode()
        self.connection.request(
            method,
            route,
            payload,
            {
                "Authorization": "Bearer " + self.token,
                "Content-Type": "application/json",
            },
        )
        response = self.connection.getresponse()
        data = response.read(16 * 1024 * 1024 + 1)
        if response.status != 200 or len(data) > 16 * 1024 * 1024:
            raise ValueError("Benchmark request failed or exceeded response size")
        if response.getheader("Cache-Control") != "no-store":
            raise ValueError("Unexpected cache contract")
        return json.loads(data), len(data)

    def close(self):
        self.connection.close()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def run(base, token, fixture, *, samples=20, sizes=(1, 10, 50, 100)):
    ids = [str(uuid.UUID(value)) for value in fixture["record_ids"]]
    if not 1 <= len(ids) <= 100 or len(set(ids)) != len(ids):
        raise ValueError("Expected 1-100 distinct record IDs")
    if samples < 2 or any(size < 1 or size > len(ids) for size in sizes):
        raise ValueError("Invalid sample count or batch size")
    scope = fixture["scope"]
    scope_query = urllib.parse.urlencode(
        {"contract_version": 1, **{k: v for k, v in scope.items() if v is not None}}
    )
    client = Client(base, token)

    def batch(selected):
        result, size = client.call(
            "POST",
            "/api/v1/memory/records/batch",
            {"contract_version": 1, "scope": scope, "record_ids": selected},
        )
        if result["scope"] != scope or result["contract_version"] != 1:
            raise ValueError("Unexpected batch identity")
        entries = result["batch"]["entries"]
        if [entry["record_id"] for entry in entries] != selected:
            raise ValueError("Incomplete, duplicated or reordered batch")
        return [entry["record"] for entry in entries], size

    def single(selected):
        records, size = [], 0
        for identifier in selected:
            result, response_bytes = client.call(
                "GET", "/api/v1/memory/records/" + identifier + "?" + scope_query
            )
            if result["scope"] != scope or result["contract_version"] != 1:
                raise ValueError("Unexpected single-read identity")
            records.append(result["record"])
            size += response_bytes
        return records, size

    try:
        frozen, _ = batch(ids)
        if any(record is None for record in frozen):
            raise ValueError(
                "The comparison requires every frozen record to be eligible"
            )
        if [record["record_id"] for record in frozen] != ids:
            raise ValueError("Unexpected record identity")
        rows = []
        for count in sizes:
            selected, expected = ids[:count], frozen[:count]
            timings = {"single": [], "batch": []}
            byte_counts = {"single": [], "batch": []}
            # Two warmups per mode, then alternate which mode runs first.
            for iteration in range(samples + 2):
                order = (
                    ("single", "batch") if iteration % 2 == 0 else ("batch", "single")
                )
                for mode in order:
                    started = time.perf_counter_ns()
                    records, response_bytes = (single if mode == "single" else batch)(
                        selected
                    )
                    elapsed = (time.perf_counter_ns() - started) / 1_000_000
                    if records != expected:
                        raise ValueError(
                            "Current records changed; comparison is invalid"
                        )
                    if iteration >= 2:
                        timings[mode].append(elapsed)
                        byte_counts[mode].append(response_bytes)
            for mode, values in timings.items():
                rows.append(
                    {
                        "records": count,
                        "mode": mode,
                        "requests_per_sample": count if mode == "single" else 1,
                        "p50_ms": statistics.median(values),
                        "p95_ms": sorted(values)[math.ceil(0.95 * len(values)) - 1],
                        "samples_ms": values,
                        "response_body_bytes": byte_counts[mode],
                    }
                )
        final, _ = batch(ids)
        if final != frozen:
            raise ValueError("Current records changed at final verification")
        return {
            "schema_version": 1,
            "complete": True,
            "fixture_sha256": hashlib.sha256(canonical(frozen).encode()).hexdigest(),
            "records": len(ids),
            "samples_per_mode_and_size": samples,
            "warmups_per_mode_and_size": 2,
            "client_concurrency": 1,
            "transport": "HTTP/1.1; one reused connection; no redirects or retries",
            "timing": "Client elapsed, including HTTP and JSON decode; equality checks excluded",
            "cache": "Warmup requests; no server or OS cache flush",
            "p95_estimator": "Nearest rank: sorted[ceil(0.95*n)-1]",
            "observed_records_equal": True,
            "rows": rows,
            "limitations": [
                "Sequential GETs are separate snapshots; corpus was held unchanged for comparison.",
                "No relevance, agent-task, server CPU, RSS, cold-cache or concurrent-load measurement.",
                "Response bytes exclude HTTP headers and transport framing.",
            ],
        }
    finally:
        client.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--token-env", default="QILBEE_TOKEN")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--sizes", type=int, nargs="+", default=[1, 10, 50, 100])
    args = parser.parse_args()
    # Reserve the artifact before any reads; never overwrite a prior campaign.
    with args.output.open("x", encoding="utf-8") as output:
        result = run(
            args.base,
            os.environ[args.token_env],
            json.loads(args.fixture.read_text()),
            samples=args.samples,
            sizes=args.sizes,
        )
        json.dump(result, output, indent=2, allow_nan=False)
        output.write("\n")


if __name__ == "__main__":
    main()
