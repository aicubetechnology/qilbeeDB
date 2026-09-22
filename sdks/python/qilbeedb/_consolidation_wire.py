"""Validate bounded source context before invoking an external extractor.

These checks validate the received contract, not independent provenance truth.
The server remains responsible for transitive eligibility and atomic publication.
"""
import math
from . import _consumer_wire as wire


def fields(value, required, optional=""):
    wire.require(isinstance(value, dict))
    wire.require(set(required.split()) <= set(value) <= set((required + " " + optional).split()))


def timestamp(value):
    return wire.integer(value, -(2**63), 2**63 - 1)


def dependency_work(value):
    wire.object_fields(value, "records_examined bytes_examined")
    wire.integer(value["records_examined"], 0, 4096)
    wire.integer(value["bytes_examined"], 0, 16 * 1024 * 1024)


def sources(value, minimum=1):
    wire.require(isinstance(value, list) and minimum <= len(value) <= 16)
    seen = set()
    for source in value:
        wire.object_fields(source, "record_id revision")
        wire.uuid(source["record_id"])
        wire.integer(source["revision"], 1)
        wire.require(source["record_id"] not in seen)
        seen.add(source["record_id"])
    return seen


def context_record(record, evaluated_at):
    fields(record, "schema_version record_id revision created_at_millis modified_at_millis author payload", "review derivation")
    wire.integer(record["schema_version"], 1, 1)
    wire.uuid(record["record_id"])
    wire.integer(record["revision"], 1)
    timestamp(record["created_at_millis"])
    timestamp(record["modified_at_millis"])
    wire.author(record["author"])
    payload = record["payload"]
    fields(payload, "episode_type content event_time_millis", "valid_until_millis tags metadata")
    kind = payload["episode_type"]
    if isinstance(kind, dict):
        wire.object_fields(kind, "Custom")
        wire.require(isinstance(kind["Custom"], str))
    else:
        wire.require(kind in ("Conversation", "TaskExecution", "Observation", "Decision", "Error"))
    timestamp(payload["event_time_millis"])
    expiry = payload.get("valid_until_millis")
    if expiry is not None:
        timestamp(expiry)
        wire.require(evaluated_at < expiry, "expired_context_record")
    wire.require(isinstance(payload.get("tags", []), list) and all(isinstance(t, str) for t in payload.get("tags", [])))
    wire.require(isinstance(payload.get("metadata", {}), dict))
    content = payload["content"]
    fields(content, "primary", "secondary context data embedding")
    wire.require(isinstance(content["primary"], str))
    for name in ("secondary", "context"):
        wire.require(content.get(name) is None or isinstance(content[name], str))
    embedding = content.get("embedding")
    if embedding is not None:
        wire.require(isinstance(embedding, list))
        wire.require(all(type(v) in (int, float) and abs(v) <= 3.4028235e38 and math.isfinite(v) for v in embedding))
    review = record.get("review")
    if review is not None:
        wire.object_fields(review, "disposition evidence_ref author reviewed_at_millis")
        wire.require(review["disposition"] in ("approved", "unreviewed"), "ineligible_context_review")
        wire.identifier(review["evidence_ref"], 2048)
        wire.author(review["author"])
        timestamp(review["reviewed_at_millis"])
    derivation = record.get("derivation")
    if derivation is not None:
        wire.object_fields(derivation, "sources method method_revision evidence_ref")
        wire.require(record["record_id"] not in sources(derivation["sources"]), "self_derived_context_record")
        wire.identifier(derivation["method"])
        wire.identifier(derivation["method_revision"])
        wire.identifier(derivation["evidence_ref"], 2048)


def job_details(job):
    spec = job["spec"]
    wire.object_fields(spec, "sources objective policy_ref extractor max_relations max_attempts lease_millis max_attempt_millis")
    wire.identifier(spec["objective"], 4096)
    wire.identifier(spec["policy_ref"], 2048)
    provenance = spec["extractor"]
    wire.object_fields(provenance, "origin method method_revision evidence_ref model")
    wire.require(provenance["origin"] in ("model_inference", "tool_observation", "human_statement", "imported_assertion"))
    wire.identifier(provenance["method"])
    wire.identifier(provenance["method_revision"])
    wire.identifier(provenance["evidence_ref"], 2048)
    model = provenance["model"]
    wire.require(provenance["origin"] != "model_inference" or model is not None)
    if model is not None:
        wire.object_fields(model, "provider model revision")
        for value in model.values():
            wire.identifier(value)
    timestamp(job["created_at_millis"])
    timestamp(job["modified_at_millis"])
    wire.require(job["modified_at_millis"] >= job["created_at_millis"])
    fences = set()
    published = 0
    for index, attempt in enumerate(job["attempts"]):
        wire.identifier(attempt["worker_id"])
        wire.require(attempt["fence"] not in fences)
        fences.add(attempt["fence"])
        claimed = timestamp(attempt["claimed_at_millis"])
        expires = timestamp(attempt["expires_at_millis"])
        hard = timestamp(claimed + spec["max_attempt_millis"])
        wire.require(job["created_at_millis"] <= claimed < expires <= hard)
        if attempt["ended_at_millis"] is not None:
            wire.require(timestamp(attempt["ended_at_millis"]) >= claimed)
        for name in ("evidence_ref", "usage_evidence_ref"):
            if attempt[name] is not None:
                wire.identifier(attempt[name], 2048)
        outcome = attempt["outcome"]
        wire.require(outcome in ("running", "published", "failed", "unknown"))
        if outcome == "running":
            wire.require(index == len(job["attempts"]) - 1 and job["status"] == "running")
            wire.require(attempt["ended_at_millis"] is None and attempt["evidence_ref"] is None and attempt["usage"] == {"status": "unknown"})
        else:
            wire.require(attempt["ended_at_millis"] is not None and attempt["evidence_ref"] is not None)
        published += outcome == "published"
    running = bool(job["attempts"]) and job["attempts"][-1]["outcome"] == "running"
    wire.require((job["status"] == "running") == running)
    receipts = job["output_receipts"]
    wire.require(isinstance(receipts, list) and len(receipts) <= spec["max_relations"])
    wire.require((published == 1) if job["status"] == "published" else (published == 0 and not receipts))
    wire.require(job["status"] != "ready" or len(job["attempts"]) < spec["max_attempts"])
    wire.require(job["status"] != "exhausted" or len(job["attempts"]) == spec["max_attempts"])
    for receipt in receipts:
        wire.object_fields(receipt, "contract_version idempotency_key relation_id revision action author committed_at_millis evidence_ref relation_digest command_digest receipt_digest")
        wire.integer(receipt["contract_version"], 1, 1)
        wire.identifier(receipt["idempotency_key"])
        wire.uuid(receipt["relation_id"])
        wire.integer(receipt["revision"], 1)
        wire.require(receipt["action"] == "asserted")
        wire.author(receipt["author"])
        wire.require(receipt["author"]["subject_id"] == job["created_by"]["subject_id"])
        timestamp(receipt["committed_at_millis"])
        wire.identifier(receipt["evidence_ref"], 2048)
        for name in ("relation_digest", "command_digest", "receipt_digest"):
            wire.digest(receipt[name])


def source_failure(value):
    if value is None:
        return
    wire.object_fields(value, "record_id expected_revision actual_revision reason")
    wire.uuid(value["record_id"])
    for name in ("expected_revision", "actual_revision"):
        if value[name] is not None:
            wire.integer(value[name], 1)
    wire.require(value["reason"] in ("deleted", "expired", "rejected", "source_missing", "source_revision_changed", "dependency_cycle", "depth_limit", "node_limit"))
