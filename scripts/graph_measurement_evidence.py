"""Validate measured graph retrieval costs before exporting conclusions."""
import math
from evaluate_retrieval import canonical


def validate_measurements(row, repetitions):
    if type(repetitions) is not int or not 1 <= repetitions <= 20:
        raise ValueError("Invalid declared repetition count")
    seen = set()
    payload_bytes = sum(len(canonical(hit["record"]["payload"])) for hit in row["hits"])
    for sample in row["samples"]:
        rep = sample["repetition"]
        if type(rep) is not int or not 0 <= rep < repetitions or rep in seen:
            raise ValueError("Invalid or duplicated measurement repetition")
        seen.add(rep)
        unavailable = sample.get("embedding_timing_status") == "unavailable_batch_capture"
        if sample.get("embedding_timing_status") not in (None, "measured", "unavailable_batch_capture"):
            raise ValueError("Unknown embedding timing status")
        if unavailable and (sample["external_query_embedding_ms"] is not None
                            or sample["embedding_plus_http_ms"] is not None
                            or row["method"] in ("lexical", "graph_lexical_balanced")):
            raise ValueError("Unavailable batch timings require paired nulls on vector methods")
        keys = ("retrieval_ms", "http_ms") if unavailable else (
            "retrieval_ms", "http_ms", "external_query_embedding_ms", "embedding_plus_http_ms")
        for key in keys:
            value = sample[key]
            try:
                valid = type(value) in (int, float) and math.isfinite(value) and value >= 0
            except OverflowError:
                valid = False
            if not valid:
                raise ValueError("Measurement must be a finite nonnegative duration: " + key)
        if sample["retrieval_ms"] > sample["http_ms"] + 1:
            raise ValueError("Retrieval time exceeds the HTTP observation tolerance")
        if not unavailable and not math.isclose(sample["embedding_plus_http_ms"],
                            sample["external_query_embedding_ms"] + sample["http_ms"],
                            rel_tol=1e-12, abs_tol=1e-9):
            raise ValueError("Combined time differs from external embedding plus HTTP time")
        if row["method"] in ("lexical", "graph_lexical_balanced") and sample["external_query_embedding_ms"] != 0:
            raise ValueError("Lexical-only measurement includes query embedding time")
        for key in ("response_bytes", "payload_bytes"):
            if type(sample[key]) is not int or sample[key] < 0:
                raise ValueError("Byte measurements must be nonnegative integers")
        if sample["payload_bytes"] != payload_bytes or payload_bytes > sample["response_bytes"]:
            raise ValueError("Measured payload bytes differ from returned evidence")
    if seen != set(range(repetitions)):
        raise ValueError("Incomplete measurement repetitions")
