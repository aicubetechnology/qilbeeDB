#!/usr/bin/env python3
"""Generate frozen evaluation vectors outside QilbeeDB using verified local E5 assets."""

import argparse
import copy
import hashlib
import importlib.metadata
import json
from pathlib import Path
import platform
import time

from evaluate_retrieval import digest, save, validate_fixture

MODEL = "intfloat/multilingual-e5-small"
REVISION = "614241f622f53c4eeff9890bdc4f31cfecc418b3"
ASSETS = {
    "model.onnx": "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665",
    "tokenizer.json": "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
}


def verify_asset(path, expected):
    checksum = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while block := stream.read(1024 * 1024):
            checksum.update(block)
    if checksum.hexdigest() != expected:
        raise ValueError("Model artifact digest mismatch: " + Path(path).name)


def mean_pool(hidden, attention_mask):
    import numpy as np

    hidden = np.asarray(hidden, dtype=np.float32)
    mask = np.asarray(attention_mask, dtype=np.float32)
    if (
        hidden.ndim != 3
        or hidden.shape[:2] != mask.shape
        or not np.isfinite(hidden).all()
    ):
        raise ValueError("Invalid encoder output")
    lengths = mask.sum(axis=1, keepdims=True)
    if (lengths <= 0).any() or not np.isin(mask, [0, 1]).all():
        raise ValueError("Invalid attention mask")
    pooled = (hidden * mask[..., None]).sum(axis=1) / lengths
    norms = np.linalg.norm(pooled, axis=1, keepdims=True)
    if not np.isfinite(norms).all() or (norms == 0).any():
        raise ValueError("Invalid pooled embedding")
    return pooled / norms


class E5Encoder:
    """Optional reference encoder: no network requests and no database credentials."""

    def __init__(self, model_dir):
        import onnxruntime as ort
        from tokenizers import Tokenizer

        model_dir = Path(model_dir)
        for name, checksum in ASSETS.items():
            verify_asset(model_dir / name, checksum)
        self.provenance = {
            "source": "https://huggingface.co/" + MODEL + "/tree/" + REVISION,
            "repository_revision": REVISION,
            "artifacts_sha256": ASSETS,
            "pipeline": "e5-mean-l2-fp32-v1",
            "query_prefix": "query: ",
            "document_prefix": "passage: ",
            "pooling": "attention-mask mean; float32 L2 normalization",
            "max_tokens": 512,
            "overlength": "reject; never truncate",
            "execution_provider": "CPUExecutionProvider",
            "intra_op_threads": 2,
            "inter_op_threads": 1,
            "graph_optimization": "ORT_ENABLE_ALL",
            "runtime": {
                name: importlib.metadata.version(name)
                for name in ["onnxruntime", "tokenizers", "numpy"]
            },
            "architecture": platform.machine(),
        }
        self.space = {
            "provider": "local-onnx-cpu",
            "model": MODEL,
            "revision": REVISION + ":" + digest(self.provenance),
            "dimensions": 384,
        }
        self.tokenizer = Tokenizer.from_file(str(model_dir / "tokenizer.json"))
        self.tokenizer.no_truncation()
        self.tokenizer.no_padding()
        options = ort.SessionOptions()
        options.intra_op_num_threads = 2
        options.inter_op_num_threads = 1
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        self.session = ort.InferenceSession(
            str(model_dir / "model.onnx"),
            sess_options=options,
            providers=["CPUExecutionProvider"],
        )
        self.inputs = {item.name for item in self.session.get_inputs()}
        if not self.inputs <= {"input_ids", "attention_mask", "token_type_ids"}:
            raise ValueError("Unsupported encoder inputs")

    def encode(self, text, kind):
        import numpy as np

        if kind not in ("document", "query"):
            raise ValueError("Unknown embedding input role")
        started = time.perf_counter()
        tokens = self.tokenizer.encode(self.provenance[kind + "_prefix"] + text)
        if len(tokens.ids) > 512:
            raise ValueError(
                "Input exceeds 512 tokens; split the source before freezing it"
            )
        values = {
            "input_ids": tokens.ids,
            "attention_mask": tokens.attention_mask,
            "token_type_ids": tokens.type_ids,
        }
        inputs = {
            name: np.asarray([values[name]], dtype=np.int64) for name in self.inputs
        }
        hidden = self.session.run(None, inputs)[0]
        vector = mean_pool(hidden, np.asarray([tokens.attention_mask]))[0]
        if vector.shape != (384,):
            raise ValueError("Unexpected encoder dimensions")
        return vector.tolist(), {
            "elapsed_ms": (time.perf_counter() - started) * 1000,
            "tokens": len(tokens.ids),
        }


def validate_source(source):
    # Validate text, judgments and splits before any model work. Vectors are inputs
    # only in the finished fixture; placeholder dimensions never leave this check.
    probe = copy.deepcopy(source)
    probe["space"] = {
        "provider": "validation",
        "model": "text-only",
        "revision": "v1",
        "dimensions": 1,
    }
    probe["embedding_provenance"] = "Validation placeholder; not a generated embedding"
    for group in ("documents", "queries"):
        for entry in probe[group]:
            entry["vector"] = [1.0]
    validate_fixture(probe)


def build_fixture(source, encoder):
    validate_source(source)
    fixture = copy.deepcopy(source)
    fixture["space"] = dict(encoder.space)
    fixture["embedding_provenance"] = copy.deepcopy(encoder.provenance)
    measurements = {
        "schema_version": 1,
        "source_sha256": digest(source),
        "model_space": dict(encoder.space),
        "documents": {},
        "queries": {},
        "timing_scope": "serial per-input tokenization, CPU inference, masked pooling and normalization; excludes asset loading, downloads and file writes; no warmup excluded",
        "environment": {
            "system": platform.system(),
            "architecture": platform.machine(),
            "python": platform.python_version(),
        },
        "total_cost": None,
        "cost_reason": "No paid provider call; local hardware and energy costs are unmeasured.",
    }
    for group, kind in [("documents", "document"), ("queries", "query")]:
        for entry in fixture[group]:
            entry["vector"], measurements[group][entry["id"]] = encoder.encode(
                entry["text"], kind
            )
    validate_fixture(fixture)
    measurements["fixture_sha256"] = digest(fixture)
    return fixture, measurements


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--measurements", type=Path, required=True)
    args = parser.parse_args()
    if (
        args.output.exists()
        or args.measurements.exists()
        or args.output == args.measurements
    ):
        parser.error(
            "Use distinct new paths; frozen fixtures and measurements are not overwritten"
        )
    source = json.loads(args.source.read_text())
    validate_source(source)
    fixture, measurements = build_fixture(source, E5Encoder(args.model_dir))
    save(args.output, fixture)
    save(args.measurements, measurements)
    print("Wrote external embedding fixture:", args.output, digest(fixture))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
