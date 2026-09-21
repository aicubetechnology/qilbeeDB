#!/usr/bin/env python3
"""Compare frozen scoped retrieval fixtures over HTTP without generating embeddings."""

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import random
import statistics
import stat
import struct
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

MODES = ("lexical", "semantic", "hybrid")
VERSIONS = {
    "lexical": "bm25_v1",
    "semantic": "cosine_exact_v1",
    "hybrid": "weighted_rrf_v1",
}

PROFILES = {
    "weighted_rrf_v1": {
        "version": "weighted_rrf_v1",
        "method": "weighted_rrf",
        "lexical_version": "bm25_v1",
        "semantic_version": "cosine_exact_v1",
        "candidate_limit": 100,
        "lexical_weight": 0.5,
        "semantic_weight": 0.5,
        "rank_constant": 60,
        "experimental": True,
    },
}

PROFILES["weighted_rrf_v2"] = dict(
    PROFILES["weighted_rrf_v1"],
    version="weighted_rrf_v2",
    lexical_weight=0.25,
    semantic_weight=0.75,
    rank_constant=2,
)


def trial_plan(
    fixture,
    split="all",
    ranking_version="weighted_rrf_v1",
    repetitions=3,
    seed=20260920,
    scan_bytes_limit=67108864,
):
    if type(scan_bytes_limit) is not int or not 1 <= scan_bytes_limit <= 268435456:
        raise ValueError("Scan byte budget must be in 1..268435456")
    if ranking_version not in PROFILES:
        raise ValueError("Unknown server-owned ranking version")
    return {
        "schema_version": 1,
        "fixture_sha256": digest(fixture),
        "split": split,
        "hybrid_profile": dict(PROFILES[ranking_version]),
        "repetitions": repetitions,
        "seed": seed,
        "k": 10,
        "scan_limit": 10000,
        "scan_bytes_limit": scan_bytes_limit,
        "min_score": -1.0,
        "client_concurrency": 1,
        "warmup_passes": 1,
    }


def validate_plan(plan, fixture):
    try:
        expected = trial_plan(
            fixture,
            plan["split"],
            plan["hybrid_profile"]["version"],
            plan["repetitions"],
            plan["seed"],
            plan["scan_bytes_limit"],
        )
        valid = (
            plan == expected
            and plan["split"] in ("development", "test", "all")
            and type(plan["repetitions"]) is int
            and 1 <= plan["repetitions"] <= 100
            and type(plan["seed"]) is int
        )
    except (KeyError, TypeError, ValueError):
        valid = False
    if not valid:
        raise ValueError(
            "Trial plan differs from its frozen fixture or supported server profile"
        )


def canonical(value):
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def save(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".retrieval-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            json.dump(value, stream, indent=2, ensure_ascii=False, allow_nan=False)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


@contextmanager
def state_lock(path):
    """Hold a cooperating-writer lock through the entire evaluation (POSIX)."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise ValueError("Evaluation state must not be a symlink")
    lock_path = path.with_name("." + path.name + ".lock")
    fd = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise ValueError("Evaluation lock must be a regular file")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("Evaluation state is already in use") from None
        yield
    finally:
        # Keep the lock inode: unlinking lets another writer lock a different inode.
        os.close(fd)


def validate_state(state, fixture):
    documents = state.get("documents")
    aliases = {d["id"] for d in fixture["documents"]}
    if not isinstance(documents, dict) or not set(documents) <= aliases:
        raise ValueError("Saved corpus document set changed")
    ids = set()
    for entry in documents.values():
        try:
            record_id = str(uuid.UUID(entry["record_id"]))
            revision = entry["revision"]
            embedding = entry["embedding"]
            valid = (
                record_id == entry["record_id"]
                and record_id not in ids
                and type(revision) is int
                and revision > 0
                and embedding["record_id"] == record_id
                and embedding["record_revision"] == revision
                and embedding["space"] == fixture["space"]
                and embedding["contract_version"] == 1
                and isinstance(embedding["vector_digest"], str)
                and len(embedding["vector_digest"]) == 64
            )
        except (KeyError, TypeError, ValueError, AttributeError):
            valid = False
        if not valid:
            raise ValueError("Invalid saved source or embedding identity")
        ids.add(record_id)


def validate_fixture(fixture):
    if fixture.get("schema_version") != 1 or fixture.get("kind") not in (
        "synthetic_contract",
        "authorized_relevance",
    ):
        raise ValueError("Unsupported fixture version or provenance kind")
    if not fixture.get("provenance") or not fixture.get("embedding_provenance"):
        raise ValueError("Corpus and embedding provenance are required")
    dimensions = fixture["space"]["dimensions"]
    if type(dimensions) is not int or not 1 <= dimensions <= 32768:
        raise ValueError("Unsupported vector dimensionality")
    for field in ("provider", "model", "revision"):
        if (
            not isinstance(fixture["space"][field], str)
            or not fixture["space"][field].strip()
        ):
            raise ValueError("Complete model identity is required")

    def vector(values):
        if len(values) != dimensions:
            raise ValueError("Vector dimensions differ from the frozen space")
        try:
            values = [struct.unpack("f", struct.pack("f", v))[0] for v in values]
        except (OverflowError, struct.error, TypeError):
            raise ValueError("Invalid float32 vector") from None
        if not all(math.isfinite(v) for v in values) or sum(v * v for v in values) == 0:
            raise ValueError("Vectors must be finite and nonzero in float32")

    documents = fixture["documents"]
    queries = fixture["queries"]
    ids = {d["id"] for d in documents}
    if not documents or len(ids) != len(documents):
        raise ValueError("Document IDs must be nonempty and unique")
    if not queries or len({q["id"] for q in queries}) != len(queries):
        raise ValueError("Query IDs must be unique")
    if {q["split"] for q in queries} != {"development", "test"}:
        raise ValueError("Separate development and test queries are required")
    development = {
        q["text"].strip().casefold() for q in queries if q["split"] == "development"
    }
    if any(
        q["text"].strip().casefold() in development
        for q in queries
        if q["split"] == "test"
    ):
        raise ValueError("Identical queries cannot cross development/test splits")
    for doc in documents:
        if (
            not isinstance(doc["id"], str)
            or not doc["id"]
            or not isinstance(doc["text"], str)
        ):
            raise ValueError("Invalid document")
        vector(doc["vector"])
    for query in queries:
        if not query["text"].strip() or not query["category"]:
            raise ValueError("Queries require text and a category")
        vector(query["vector"])
        judgments = query["judgments"]
        if not set(judgments) <= ids:
            raise ValueError("Judgments refer to unknown documents")
        if any(type(g) is not int or not 0 <= g <= 3 for g in judgments.values()):
            raise ValueError("Relevance grades must be integers in 0..3")
        if type(query.get("judgments_complete")) is not bool:
            raise ValueError("Judgment coverage must be explicit")
        if query["judgments_complete"] and set(judgments) != ids:
            raise ValueError("Complete judgments must explicitly grade every document")


def metrics(ranked, query, k=10):
    ranked = ranked[:k]
    if len(set(ranked)) != len(ranked):
        raise ValueError("Duplicate retrieval results")
    judgments = query["judgments"]
    relevant = {d for d, g in judgments.items() if g > 0}

    def dcg(grades):
        return sum(
            (2**grade - 1) / math.log2(rank + 2) for rank, grade in enumerate(grades)
        )

    ideal = dcg(sorted(judgments.values(), reverse=True)[:k])
    ndcg = dcg([judgments.get(d, 0) for d in ranked]) / ideal if ideal else None
    recall = len(relevant.intersection(ranked)) / len(relevant) if relevant else None
    return {
        "ndcg_at_10": ndcg,
        "recall_at_10": recall if query["judgments_complete"] else None,
        "judged_recall_at_10": recall,
        "judgments_complete": query["judgments_complete"],
        "judged_result_fraction": (
            sum(d in judgments for d in ranked) / len(ranked) if ranked else None
        ),
        "answerable": bool(relevant),
        "no_useful_result": not bool(relevant.intersection(ranked)),
        "empty_result": not ranked,
        "returned_on_unanswerable": (
            bool(ranked) if not relevant and query["judgments_complete"] else None
        ),
    }


def quantile(values, probability):
    if not values:
        return None
    values = sorted(values)
    position = (len(values) - 1) * probability
    lo = math.floor(position)
    hi = math.ceil(position)
    return (
        values[lo] * (hi - position) + values[hi] * (position - lo)
        if hi != lo
        else values[lo]
    )


def paired_interval(deltas, seed):
    if not deltas:
        return {"mean": None, "low": None, "high": None, "queries": 0}
    rng = random.Random(seed)
    samples = [statistics.mean(rng.choices(deltas, k=len(deltas))) for _ in range(2000)]
    return {
        "mean": statistics.mean(deltas),
        "low": quantile(samples, 0.025),
        "high": quantile(samples, 0.975),
        "queries": len(deltas),
        "method": "paired query bootstrap, 2000 draws, percentile 95%; exploratory, no multiplicity correction",
    }


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Client:
    def __init__(self, base, secret):
        parsed = urllib.parse.urlparse(base)
        if (
            parsed.scheme not in ("http", "https")
            or not parsed.netloc
            or parsed.username
            or parsed.password
            or parsed.query
            or parsed.fragment
        ):
            raise ValueError(
                "Expected an HTTP(S) API base URL without credentials or query parameters"
            )
        if parsed.scheme == "http" and parsed.hostname not in (
            "localhost",
            "127.0.0.1",
            "::1",
        ):
            raise ValueError("Use HTTPS outside the local loopback interface")
        self.base = base.rstrip("/")
        self.secret = secret
        self.opener = urllib.request.build_opener(NoRedirect)

    def call(self, method, path, body=None):
        request = urllib.request.Request(
            self.base + path,
            data=canonical(body) if body is not None else None,
            method=method,
            headers={
                "Authorization": "Bearer " + self.secret,
                "Content-Type": "application/json",
            },
        )
        start = time.perf_counter_ns()
        try:
            with self.opener.open(request, timeout=30) as response:
                raw = response.read()
                elapsed = (time.perf_counter_ns() - start) / 1e6
                body = json.loads(raw)
                if path == "/api/v1/memory/search":
                    # Legacy cosine keeps its 0.4.0 JSON envelope; metadata uses headers.
                    body["mode"] = "semantic"
                    body["ranking_version"] = response.headers.get(
                        "X-Qilbee-Ranking-Version"
                    )
                    body["timing"] = {
                        "retrieval_micros": int(
                            response.headers["X-Qilbee-Retrieval-Micros"]
                        )
                    }
                if path == "/api/v1/memory/search/graph":
                    body["mode"] = "graph"
                    body["ranking_version"] = body["page"]["ranking"]["version"]
                    body["timing"] = {"retrieval_micros": int(response.headers["X-Qilbee-Retrieval-Micros"])}
                return body, elapsed, len(raw)
        except urllib.error.HTTPError as error:
            raise RuntimeError(
                f'HTTP {error.code} from {method} {path.split("?")[0]}'
            ) from None


def source_payload(document, tag, fixture_hash):
    return {
        "episode_type": "Observation",
        "content": {
            "primary": document["text"],
            "secondary": None,
            "context": None,
            "data": None,
            "embedding": None,
        },
        "event_time_millis": 1700000000000,
        "valid_until_millis": None,
        "tags": [tag],
        "metadata": {
            "fixture_sha256": fixture_hash,
            "fixture_document_id": document["id"],
        },
    }


def prepare(client, fixture, scope, state_path):
    fixture_hash = digest(fixture)
    tag = "retrieval-fixture-" + fixture_hash
    identity = client.call("GET", "/api/v1/identity")[0]["credential"]
    expected = {
        "fixture_sha256": fixture_hash,
        "scope": scope,
        "tenant_id": identity["tenant_id"],
        "subject_id": identity["spec"]["subject_id"],
    }
    if Path(state_path).exists():
        state = json.loads(Path(state_path).read_text())
        if any(state.get(k) != v for k, v in expected.items()):
            raise ValueError(
                "Saved state belongs to another corpus, scope, tenant or subject"
            )
    else:
        state = dict(expected, documents={})
        save(state_path, state)
    validate_state(state, fixture)
    # Check existing partial work before issuing any further create/attach commands.
    existing = dict(
        fixture,
        documents=[d for d in fixture["documents"] if d["id"] in state["documents"]],
    )
    verify_sources(client, existing, state, tag)
    pending = 0
    for doc in fixture["documents"]:
        alias = doc["id"]
        if alias not in state["documents"]:
            key = "benchmark-" + fixture_hash + "-" + digest(alias)[:24]
            receipt = client.call(
                "POST",
                "/api/v1/memory/commands",
                {
                    "contract_version": 1,
                    "scope": scope,
                    "idempotency_key": key,
                    "operation": {
                        "type": "create",
                        "record": source_payload(doc, tag, fixture_hash),
                    },
                },
            )[0]["receipt"]
            embedding = client.call(
                "POST",
                "/api/v1/memory/embeddings",
                {
                    "contract_version": 1,
                    "scope": scope,
                    "idempotency_key": key,
                    "record_id": receipt["record_id"],
                    "record_revision": receipt["revision"],
                    "space": fixture["space"],
                    "vector": doc["vector"],
                },
            )[0]["receipt"]
            state["documents"][alias] = {
                "record_id": receipt["record_id"],
                "revision": receipt["revision"],
                "embedding": embedding,
            }
            pending += 1
            if pending == 64:
                save(state_path, state)
                pending = 0
    if set(state["documents"]) != {d["id"] for d in fixture["documents"]}:
        raise ValueError("Saved corpus document set changed")
    if pending:
        save(state_path, state)
    return state, tag


def verify_sources(client, fixture, state, tag):
    params = dict(state["scope"], contract_version=1)
    params = {k: v for k, v in params.items() if v is not None}
    for doc in fixture["documents"]:
        frozen = state["documents"][doc["id"]]
        record = client.call(
            "GET",
            "/api/v1/memory/records/"
            + frozen["record_id"]
            + "?"
            + urllib.parse.urlencode(params),
        )[0]["record"]
        if (
            record["record_id"] != frozen["record_id"]
            or record["revision"] != frozen["revision"]
            or record["payload"] != source_payload(doc, tag, state["fixture_sha256"])
        ):
            raise ValueError("Frozen source revision or content changed")


def container_resources(container):
    if not container:
        return {"available": False, "reason": "No container supplied"}
    try:
        outputs = {}
        for name in ["cpu.stat", "memory.current", "memory.peak"]:
            result = subprocess.run(
                ["docker", "exec", container, "cat", "/sys/fs/cgroup/" + name],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
            )
            outputs[name] = result.stdout.strip()
        cpu = dict(line.split() for line in outputs["cpu.stat"].splitlines())
        return {
            "available": True,
            "cpu_usage_usec": int(cpu["usage_usec"]),
            "memory_current_bytes": int(outputs["memory.current"]),
            "memory_peak_since_container_start_bytes": int(outputs["memory.peak"]),
        }
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        return {"available": False, "reason": "Container cgroup v2 metrics unavailable"}


def query_request(
    mode,
    query,
    fixture,
    scope,
    tag,
    ranking_version="weighted_rrf_v1",
    scan_bytes_limit=67108864,
):
    fields = {"limit": 10, "scan_limit": 10000, "tag": tag}
    if mode != "semantic":
        fields.update(text=query["text"], scan_bytes_limit=scan_bytes_limit)
    if mode != "lexical":
        fields.update(space=fixture["space"], vector=query["vector"], min_score=-1.0)
    if mode == "hybrid":
        fields["ranking_version"] = ranking_version
    body = {"contract_version": 1, "scope": scope, "query": fields}
    if mode != "semantic":
        body["mode"] = mode
    path = "/api/v1/memory/search" + ("/" + mode if mode != "semantic" else "")
    return path, body


def summarize(rows):
    def mean(key, selection=rows):
        values = [
            row["metrics"][key] for row in selection if row["metrics"][key] is not None
        ]
        return statistics.mean(values) if values else None

    answerable = [r for r in rows if r["metrics"]["answerable"]]
    return {
        "queries": len(rows),
        "ndcg_at_10": mean("ndcg_at_10"),
        "recall_at_10": mean("recall_at_10"),
        "judged_recall_at_10": mean("judged_recall_at_10"),
        "answerable_no_useful_result_rate": mean("no_useful_result", answerable),
        "unanswerable_return_rate": mean("returned_on_unanswerable"),
        "empty_result_rate": mean("empty_result"),
        "all_judgments_complete": all(r["metrics"]["judgments_complete"] for r in rows),
        "retrieval_ms": {
            key: quantile([s["retrieval_ms"] for r in rows for s in r["samples"]], p)
            for key, p in [("p50", 0.5), ("p95", 0.95)]
        },
        "http_ms": {
            key: quantile([s["http_ms"] for r in rows for s in r["samples"]], p)
            for key, p in [("p50", 0.5), ("p95", 0.95)]
        },
        "mean_response_bytes": (
            statistics.mean(s["response_bytes"] for r in rows for s in r["samples"])
            if rows
            else None
        ),
    }


def evaluate(
    client, fixture, scope, state_path, repetitions, seed, container=None, plan=None
):
    validate_fixture(fixture)
    plan = (
        plan
        if plan is not None
        else trial_plan(fixture, repetitions=repetitions, seed=seed)
    )
    validate_plan(plan, fixture)
    with state_lock(state_path):
        return evaluate_locked(client, fixture, scope, state_path, plan, container)


def evaluate_locked(client, fixture, scope, state_path, plan, container=None):
    repetitions, seed = plan["repetitions"], plan["seed"]
    ranking_version = plan["hybrid_profile"]["version"]
    versions = dict(VERSIONS, hybrid=ranking_version)
    queries = [
        q
        for q in fixture["queries"]
        if plan["split"] == "all" or q["split"] == plan["split"]
    ]
    state, tag = prepare(client, fixture, scope, state_path)
    verify_sources(client, fixture, state, tag)
    frozen = {
        entry["record_id"]: (alias, entry)
        for alias, entry in state["documents"].items()
    }
    health = client.call("GET", "/health")[0]
    rows = {}
    failures = []
    violations = {
        key: 0
        for key in [
            "scope_or_corpus",
            "stale_revision",
            "duplicate_results",
            "partial_scan",
            "partial_embedding_coverage",
            "unstable_ranking",
        ]
    }
    rng = random.Random(seed)
    before = None
    for repetition in range(-1, repetitions):
        if repetition == 0:
            before = container_resources(container)
        jobs = [(query, mode) for query in queries for mode in MODES]
        rng.shuffle(jobs)
        for query, mode in jobs:
            path, body = query_request(
                mode,
                query,
                fixture,
                scope,
                tag,
                ranking_version,
                plan["scan_bytes_limit"],
            )
            try:
                result, elapsed, size = client.call("POST", path, body)
                page = result["page"]
                if (
                    result["scope"] != scope
                    or result["ranking_version"] != versions[mode]
                    or result["mode"] != mode
                ):
                    violations["scope_or_corpus"] += 1
                    raise ValueError("Scope or ranking identity changed")
                if mode == "hybrid" and page.get("ranking") != plan["hybrid_profile"]:
                    violations["scope_or_corpus"] += 1
                    raise ValueError(
                        "Server ranking profile differs from the pinned plan"
                    )
                if page["exhaustive"] is not True or page["next_after"] is not None:
                    violations["partial_scan"] += 1
                    raise ValueError("Evaluation requires complete corpus scans")
                count = (
                    page["matched_records"]
                    if mode == "semantic"
                    else page["corpus_records"]
                )
                if count != len(frozen):
                    violations["scope_or_corpus"] += 1
                    raise ValueError("Frozen corpus coverage changed")
                if mode == "hybrid" and page["embedding_coverage"] != "complete":
                    violations["partial_embedding_coverage"] += 1
                    raise ValueError(
                        "Evaluation requires current vectors for every source"
                    )
                ids = [hit["record"]["record_id"] for hit in page["hits"]]
                if len(ids) != len(set(ids)):
                    violations["duplicate_results"] += 1
                    raise ValueError("Duplicate results")
                if not set(ids) <= set(frozen):
                    violations["scope_or_corpus"] += 1
                    raise ValueError("A result is outside the frozen authorized corpus")
                for hit in page["hits"]:
                    alias, entry = frozen[hit["record"]["record_id"]]
                    if hit["record"]["revision"] != entry["revision"]:
                        violations["stale_revision"] += 1
                        raise ValueError("A source revision changed")
                    if hit.get("embedding") and hit["embedding"] != entry["embedding"]:
                        violations["stale_revision"] += 1
                        raise ValueError("Embedding evidence changed")
                if repetition < 0:
                    continue
                aliases = [frozen[id][0] for id in ids]
                key = (query["id"], mode)
                sample = {
                    "retrieval_ms": result["timing"]["retrieval_micros"] / 1000,
                    "http_ms": elapsed,
                    "response_bytes": size,
                }
                if key not in rows:
                    counts = {
                        k: page[k]
                        for k in [
                            "scanned_records",
                            "scanned_embeddings",
                            "scanned_bytes",
                            "corpus_records",
                            "embedded_records",
                            "matched_records",
                            "lexical_matches",
                            "semantic_matches",
                            "lexical_candidates",
                            "semantic_candidates",
                            "candidates_truncated",
                        ]
                        if k in page
                    }
                    rows[key] = {
                        "query_id": query["id"],
                        "split": query["split"],
                        "category": query["category"],
                        "mode": mode,
                        "ranking_version": result["ranking_version"],
                        "ranked": aliases,
                        "metrics": metrics(aliases, query),
                        "counts": counts,
                        "samples": [],
                        "ranking_profile": page.get("ranking"),
                        "score_evidence": [
                            {
                                k: v
                                for k, v in hit.items()
                                if k in ("score", "lexical", "semantic")
                            }
                            for hit in page["hits"]
                        ],
                    }
                elif rows[key]["ranked"] != aliases:
                    violations["unstable_ranking"] += 1
                    raise ValueError(
                        "Ranking changed across repetitions on the same frozen state"
                    )
                rows[key]["samples"].append(sample)
            except (RuntimeError, ValueError, KeyError, TypeError, OSError) as error:
                failures.append(
                    {
                        "query_id": query["id"],
                        "mode": mode,
                        "repetition": repetition,
                        "error": str(error),
                    }
                )
    after = container_resources(container)
    resources_by_mode = {}
    if container:
        for mode in MODES:
            start_resources = container_resources(container)
            started = time.perf_counter()
            try:
                for query in queries:
                    path, body = query_request(
                        mode,
                        query,
                        fixture,
                        scope,
                        tag,
                        ranking_version,
                        plan["scan_bytes_limit"],
                    )
                    result, _, _ = client.call("POST", path, body)
                    if result["page"]["exhaustive"] is not True:
                        raise ValueError("Resource pass encountered a partial scan")
            except (RuntimeError, ValueError, KeyError, OSError) as error:
                failures.append(
                    {"stage": "resource_pass", "mode": mode, "error": str(error)}
                )
            end_resources = container_resources(container)
            resources_by_mode[mode] = {
                "queries": len(queries),
                "elapsed_seconds_including_telemetry": time.perf_counter() - started,
                "before": start_resources,
                "after": end_resources,
                "cpu_usage_usec_delta": (
                    end_resources["cpu_usage_usec"] - start_resources["cpu_usage_usec"]
                    if start_resources.get("available")
                    and end_resources.get("available")
                    else None
                ),
            }
    try:
        verify_sources(client, fixture, state, tag)
    except (RuntimeError, ValueError, KeyError, OSError) as error:
        violations["stale_revision"] += 1
        failures.append({"stage": "final_source_verification", "error": str(error)})
    rows = list(rows.values())
    complete = (
        not failures
        and not any(violations.values())
        and len(rows) == len(queries) * len(MODES)
        and all(len(r["samples"]) == repetitions for r in rows)
    )
    summaries = {
        split: {
            mode: summarize(
                [r for r in rows if r["split"] == split and r["mode"] == mode]
            )
            for mode in MODES
        }
        for split in ["development", "test"]
    }
    categories = {
        category: {
            mode: summarize(
                [
                    r
                    for r in rows
                    if r["split"] == "test"
                    and r["category"] == category
                    and r["mode"] == mode
                ]
            )
            for mode in MODES
        }
        for category in sorted({q["category"] for q in queries if q["split"] == "test"})
    }
    paired = {}
    losses = []
    for baseline in ["lexical", "semantic"]:
        base = {
            r["query_id"]: r
            for r in rows
            if r["split"] == "test" and r["mode"] == baseline
        }
        deltas = []
        for row in rows:
            if (
                row["split"] != "test"
                or row["mode"] != "hybrid"
                or row["query_id"] not in base
            ):
                continue
            a = row["metrics"]["ndcg_at_10"]
            b = base[row["query_id"]]["metrics"]["ndcg_at_10"]
            if a is None or b is None:
                continue
            deltas.append(a - b)
            if a < b:
                losses.append(
                    {
                        "query_id": row["query_id"],
                        "category": row["category"],
                        "baseline": baseline,
                        "ndcg_delta": a - b,
                        "hybrid": row["ranked"],
                        "baseline_ranking": base[row["query_id"]]["ranked"],
                    }
                )
        paired[baseline] = paired_interval(deltas, seed)
    report = {
        "schema_version": 1,
        "trial_plan": plan,
        "trial_plan_sha256": digest(plan),
        "server_health": health,
        "fixture_sha256": digest(fixture),
        "fixture_kind": fixture["kind"],
        "provenance": fixture["provenance"],
        "embedding_provenance": fixture["embedding_provenance"],
        "model_space": fixture["space"],
        "manifest_sha256": digest(state),
        "scope_sha256": digest(scope),
        "source_revisions": {
            alias: {k: entry[k] for k in ("record_id", "revision")}
            | {"vector_digest": entry["embedding"]["vector_digest"]}
            for alias, entry in state["documents"].items()
        },
        "conditions": {
            "selected_split": plan["split"],
            "k": 10,
            "scan_records": 10000,
            "lexical_hybrid_scan_bytes": plan["scan_bytes_limit"],
            "limits_kind": "experimental trial budgets within implementation ceilings; not tenant policy",
            "client_concurrency": 1,
            "repetitions": repetitions,
            "seed": seed,
            "order": "seeded shuffle each pass",
            "cache": "one warmup pass; RocksDB/OS caches not flushed; no retrieval-result cache",
            "corpus_immutable_before_and_after": not any(
                f.get("stage") == "final_source_verification" for f in failures
            ),
            "ranking_parameters": "immutable server profiles; no tuning on test queries",
        },
        "metrics_definition": {
            "relevance_grades": (
                fixture["provenance"].get(
                    "judgments", "See fixture judgment provenance"
                )
                if isinstance(fixture["provenance"], dict)
                else "0 irrelevant, 1 marginal, 2 useful, 3 directly resolves the information need"
            ),
            "ndcg": "DCG@10 = sum((2^grade-1)/log2(rank+1)); divide by ideal DCG from judgments; null if no relevant judged source",
            "recall": "grade > 0; exhaustive recall only when every corpus record is explicitly judged; otherwise report judged recall separately",
            "unjudged": "treated as gain zero for judged nDCG; expose coverage",
            "no_answer": "unanswerable queries excluded from nDCG/recall averages and reported through returned/empty rates",
        },
        "valid_comparison": complete,
        "violations": violations,
        "failures": failures,
        "summary": summaries,
        "test_categories": categories,
        "paired_test_ndcg_delta": paired,
        "hybrid_losses": losses,
        "rows": rows,
        "resources": {
            "per_method_separate_warm_pass": resources_by_mode,
            "before": before,
            "after": after,
            "cpu_usage_usec_delta": (
                after["cpu_usage_usec"] - before["cpu_usage_usec"]
                if before and before.get("available") and after.get("available")
                else None
            ),
            "scope": "entire container including telemetry commands and any concurrent clients; memory peak is since container start, not isolated trial peak",
        },
        "embedding_measurements": {
            "generation_latency_ms": None,
            "generation_cost": None,
            "end_to_end_with_generation_ms": None,
            "reason": "Frozen externally supplied vectors are reused. No model invocation occurs in this trial; do not interpret missing generation measurements as zero.",
        },
        "agent_task_evaluation": {
            "status": "not_run",
            "reason": "A fixed model, prompt, tools, task suite and acceptance outcomes are required separately.",
        },
        "qualification": {
            "hybrid_admitted": False,
            "status": "experimental",
            "reason": (
                "Synthetic contract evidence cannot qualify language relevance. Review held-out gains, uncertainty, exact-query regressions and separate agent-task outcomes before adoption."
                if fixture["kind"] == "synthetic_contract"
                else "This evaluator records evidence, not an automatic production promotion. Evaluate representativeness, paired uncertainty, critical category losses and separate agent-task outcomes."
            ),
        },
    }
    return report


def markdown_report(report):
    def number(value):
        return "unavailable" if value is None else f"{value:.4f}"

    selected = report.get("conditions", {}).get("selected_split", "all")
    display_split = "development" if selected == "development" else "test"
    lines = [
        "# Retrieval evaluation report",
        "",
        f"Selected split: **{selected}**. Empty split summaries mean not run, never zero relevance.",
        "",
        "This is a "
        + report["fixture_kind"]
        + " comparison. Hybrid remains **experimental**.",
        "",
        "| Method | nDCG@10 | Recall@10 | Judged Recall@10 | No useful result, answerable | Retrieval p50 / p95 ms | HTTP p50 / p95 ms | Mean response bytes |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for mode, summary in report["summary"][display_split].items():
        lines.append(
            "| "
            + mode
            + " | "
            + number(summary["ndcg_at_10"])
            + " | "
            + number(summary["recall_at_10"])
            + " | "
            + number(summary["judged_recall_at_10"])
            + " | "
            + number(summary["answerable_no_useful_result_rate"])
            + " | "
            + number(summary["retrieval_ms"]["p50"])
            + " / "
            + number(summary["retrieval_ms"]["p95"])
            + " | "
            + number(summary["http_ms"]["p50"])
            + " / "
            + number(summary["http_ms"]["p95"])
            + " | "
            + number(summary["mean_response_bytes"])
            + " |"
        )
    lines += [
        "",
        "## Per-category held-out results",
        "",
        "| Category | Queries per method | Lexical nDCG@10 | Semantic nDCG@10 | Hybrid nDCG@10 |",
        "| --- | --- | --- | --- | --- |",
    ]
    for category, methods in report["test_categories"].items():
        lines.append(
            "| "
            + category
            + " | "
            + str(methods["hybrid"]["queries"])
            + " | "
            + " | ".join(number(methods[m]["ndcg_at_10"]) for m in MODES)
            + " |"
        )
    lines += ["", "## Paired uncertainty and losses", ""]
    for baseline, interval in report["paired_test_ndcg_delta"].items():
        lines.append(
            f"- Hybrid minus {baseline}: {number(interval['mean'])}; exploratory 95% paired bootstrap interval [{number(interval['low'])}, {number(interval['high'])}], {interval['queries']} answerable test queries."
        )
    lines.append(
        "- Hybrid losses: "
        + str(len(report["hybrid_losses"]))
        + ". The JSON report lists every losing query and both rankings."
    )
    lines += [
        "",
        "## Conditions and limits",
        "",
        f"- Valid complete comparison: {str(report['valid_comparison']).lower()}; violations: `{json.dumps(report['violations'],sort_keys=True)}`.",
        f"- Corpus: `{report['fixture_sha256']}`; saved manifest: `{report['manifest_sha256']}`.",
        "- Reuse the saved state and unchanged database to preserve generated UUIDs and deterministic tie ordering. A fresh database assigns different IDs.",
        "- One client, one warmup pass, frozen externally supplied vectors. Server retrieval time excludes authentication, queueing, serialization and transport; HTTP time includes the request/response round trip.",
        "- Full generation-to-retrieval time and embedding cost are unmeasured. No provider was invoked.",
        "- Container resource counters and all candidate/response measurements are recorded in JSON; unavailable measurements are null, not zero.",
        (
            "- The synthetic smoke corpus is not a representative language benchmark. Small category samples and bootstrap intervals do not establish production gains."
            if report["fixture_kind"] == "synthetic_contract"
            else "- External qrels cover known judgments, not exhaustive relevance. Judged recall is not exhaustive recall. A domain benchmark does not establish production or agent-task gains."
        ),
        "- No agent-task comparison was run. Better retrieval alone does not demonstrate better reasoning or autonomous improvement.",
        "",
    ]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--credential-file", type=Path)
    parser.add_argument("--scope-file", type=Path)
    parser.add_argument("--state", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--base-url", default="http://127.0.0.1:7474")
    parser.add_argument("--repetitions", type=int)
    parser.add_argument("--seed", type=int)
    parser.add_argument("--split", choices=["all", "development", "test"])
    parser.add_argument("--ranking-version", choices=sorted(PROFILES))
    parser.add_argument("--scan-bytes-limit", type=int)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--write-plan", type=Path)
    group.add_argument("--plan", type=Path)
    parser.add_argument("--container")
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    validate_fixture(fixture)
    if args.plan and any(
        v is not None
        for v in (
            args.split,
            args.ranking_version,
            args.repetitions,
            args.seed,
            args.scan_bytes_limit,
        )
    ):
        parser.error("A pinned --plan cannot be overridden with trial parameters")
    plan = (
        json.loads(args.plan.read_text())
        if args.plan
        else trial_plan(
            fixture,
            args.split or "all",
            args.ranking_version or "weighted_rrf_v1",
            args.repetitions if args.repetitions is not None else 3,
            args.seed if args.seed is not None else 20260920,
            args.scan_bytes_limit if args.scan_bytes_limit is not None else 67108864,
        )
    )
    validate_plan(plan, fixture)
    if args.write_plan:
        if args.write_plan.exists():
            parser.error("Plan already exists; use a new path for a new trial")
        save(args.write_plan, plan)
        print("Wrote trial plan:", args.write_plan, digest(plan))
        return 0
    if not all((args.credential_file, args.scope_file, args.state, args.report)):
        parser.error(
            "A run requires --credential-file, --scope-file, --state and --report"
        )
    scope = json.loads(args.scope_file.read_text())
    secret = json.loads(args.credential_file.read_text())["secret"]
    report = evaluate(
        Client(args.base_url, secret),
        fixture,
        scope,
        args.state,
        plan["repetitions"],
        plan["seed"],
        args.container,
        plan=plan,
    )
    report["fixture_file_sha256"] = hashlib.sha256(
        args.fixture.read_bytes()
    ).hexdigest()
    save(args.report, report)
    args.report.with_suffix(".md").write_text(markdown_report(report), encoding="utf-8")
    print(
        "Wrote retrieval comparison:",
        args.report,
        "; valid comparison:",
        report["valid_comparison"],
        "; hybrid status: experimental",
    )
    return 0 if report["valid_comparison"] else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print("Retrieval evaluation failed:", str(error))
        raise SystemExit(1)
