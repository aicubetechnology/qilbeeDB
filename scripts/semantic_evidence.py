"""Validate returned cosine evidence against externally supplied frozen vectors."""
import math
import struct


def frozen_cosine(left, right):
    if len(left) != len(right) or not left:
        raise ValueError("Frozen vectors must have equal nonzero dimensions")
    vectors = []
    for values in (left, right):
        try:
            if any(type(v) not in (int, float) for v in values):
                raise ValueError("Frozen vector components must be numbers")
            converted = [struct.unpack("<f", struct.pack("<f", v))[0] for v in values]
        except (OverflowError, struct.error, TypeError):
            raise ValueError("Invalid frozen float32 vector") from None
        if not all(math.isfinite(v) for v in converted):
            raise ValueError("Frozen vectors must be finite")
        norm = math.sqrt(sum(v * v for v in converted))
        if norm == 0:
            raise ValueError("Frozen vectors must be nonzero")
        vectors.append((converted, norm))
    (a, an), (b, bn) = vectors
    return max(-1.0, min(1.0, sum(x * y for x, y in zip(a, b)) / (an * bn)))


def validate_semantic_evidence(hit, method, binding, query_vector, source_vector):
    """Check the returned channel only; do not infer omitted candidates or ranks."""
    if method == "semantic":
        score = hit.get("score")
    elif method == "hybrid" or method.startswith("weighted_rrf_"):
        channel = hit.get("semantic")
        if channel is None:
            if hit.get("embedding") is not None:
                raise ValueError("Embedding receipt has no semantic contribution")
            return
        if not isinstance(channel, dict):
            raise ValueError("Malformed semantic contribution")
        score = channel.get("score")
    else:
        return
    if hit.get("embedding") != binding.get("embedding") or binding.get("embedding") is None:
        raise ValueError("Missing or altered frozen embedding receipt")
    try:
        valid = type(score) in (int, float) and math.isfinite(score) and -1 <= score <= 1
    except OverflowError:
        valid = False
    if not valid:
        raise ValueError("Cosine evidence must be a finite number in [-1, 1]")
    expected = frozen_cosine(query_vector, source_vector)
    if not math.isclose(score, expected, rel_tol=2e-6, abs_tol=1e-9):
        raise ValueError("Cosine evidence differs from the frozen float32 vectors")
