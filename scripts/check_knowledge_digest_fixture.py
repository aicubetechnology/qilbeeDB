#!/usr/bin/env python3
"""Independently verify the Rust knowledge proposal digest interoperability vector.

This checks the fixture serialization contract, not arbitrary proposal validity.
It deliberately preserves unsigned integers and Unicode without normalization.
"""
import hashlib
import json
from pathlib import Path
from uuid import UUID

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "crates/qilbee-memory/src/learning/store/knowledge/proposal-digest-v2.json"


def main() -> None:
    fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
    source = fixture["input"]
    canonical = {
        key: source[key]
        for key in ("id", "policy_id", "context_id", "title", "instructions")
    }
    canonical["memory_sources"] = [
        {"record_id": str(UUID(item["record_id"])), "revision": item["revision"]}
        for item in sorted(source["memory_sources"], key=lambda item: UUID(item["record_id"]).int)
    ]
    canonical["external_tools"] = [
        {key: item[key] for key in (
            "name", "schema_revision", "implementation_revision",
            "environment_revision", "usage_contract",
        )}
        for item in sorted(source["external_tools"], key=lambda item: item["name"])
    ]
    encoded = json.dumps(canonical, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
    if encoded != fixture["canonical_json"]:
        raise ValueError("Canonical proposal bytes differ")
    if hashlib.sha256(encoded.encode("utf-8")).hexdigest() != fixture["sha256"]:
        raise ValueError("Proposal SHA-256 differs")
    if canonical["memory_sources"][1]["revision"] != 2**64 - 1:
        raise ValueError("Unsigned source revision lost precision")
    receipt_fixture = json.loads(FIXTURE.with_name("receipt-digest-v2.json").read_text(encoding="utf-8"))
    receipt = receipt_fixture["receipt"]
    record = receipt["record"]
    # Explicit field order mirrors the documented tuple and native structs;
    # never rely on the incoming JSON object's order.
    def ordered(value, fields):
        return {key: value[key] for key in fields.split()}

    policy = ordered(record["proposal"]["policy"],
        "qualification_trials confidence_delta min_improvement min_candidate_utility "
        "max_cost_units max_latency_ms max_failure_streak evaluator_id evaluation_contract")
    procedure = ordered(record["proposal"], "id task baseline_revision instructions source_refs policy")
    procedure["policy"] = policy
    original = ordered(record,
        "scope proposal state qualification_count mean_improvement mean_candidate_utility "
        "budget_violations lower_improvement_bound monitoring_count failure_streak decisions created_at_millis")
    original["scope"] = ordered(record["scope"], "tenant agent environment")
    original["proposal"] = procedure
    if receipt["request"] != canonical or original["state"] != "Candidate" or original["decisions"]:
        raise ValueError("Unexpected original candidate fixture")
    tuple_fields = [receipt[key] for key in ("schema_version", "tenant", "namespace")]
    tuple_fields += [canonical, receipt["policy_digest"], receipt["context_digest"], receipt["actor"], original]
    receipt_bytes = json.dumps(tuple_fields, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
    if receipt_bytes != receipt_fixture["canonical_json"]:
        raise ValueError("Canonical receipt bytes differ")
    digest = hashlib.sha256(receipt_bytes.encode("utf-8")).hexdigest()
    if digest != receipt_fixture["sha256"] or digest != receipt["receipt_digest"]:
        raise ValueError("Receipt SHA-256 differs")
    print("Verified knowledge v2 proposal and original receipt bytes and SHA-256 in Python")


if __name__ == "__main__":
    main()
