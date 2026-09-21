"""Closed v2 consumer contracts. Digests are opaque; only the server verifies history."""

from dataclasses import dataclass
from uuid import UUID
import re
import unicodedata


class ConsumerError(Exception):
    """A stopped operation; checkpoint_outcome never describes external sink effects."""

    def __init__(self, code, *, status=None, checkpoint_outcome="not_attempted"):
        self.code = code
        self.status = status
        self.checkpoint_outcome = checkpoint_outcome
        super().__init__("Memory consumer stopped: " + code)


class ConsumerProtocolError(ConsumerError):
    pass


class ConsumerAPIError(ConsumerError):
    pass


class ConsumerTransportError(ConsumerError):
    pass


class ReconciliationRequired(ConsumerError):
    pass


def require(condition, code="invalid_response"):
    if not condition:
        raise ConsumerProtocolError(code)


def object_fields(value, fields):
    require(isinstance(value, dict) and set(value) == set(fields.split()))
    return value


def integer(value, minimum=0, maximum=2**64 - 1):
    require(type(value) is int and minimum <= value <= maximum)
    return value


def identifier(value, maximum=256):
    require(isinstance(value, str) and bool(value.strip()))
    require(len(value.encode("utf-8")) <= maximum)
    require(not any(unicodedata.category(c) == "Cc" for c in value))
    return value


def digest(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None)
    return value


def uuid(value):
    require(isinstance(value, str))
    try:
        require(str(UUID(value)) == value)
    except (ValueError, AttributeError):
        raise ConsumerProtocolError("invalid_response") from None
    return value


def scope(value):
    require(isinstance(value, dict))
    value = dict(value)
    value.setdefault("mission_id", None)
    object_fields(value, "project_id mission_id agent_id visibility")
    identifier(value["project_id"])
    identifier(value["agent_id"])
    if value["mission_id"] is not None:
        identifier(value["mission_id"])
    require(value["visibility"] in ("shared", "private"))
    return value


@dataclass(frozen=True)
class VerifiedCursor:
    version: int
    journal_id: str
    generation: str
    sequence: int
    prefix_digest: str

    def __post_init__(self):
        require(type(self.version) is int and self.version == 2)
        uuid(self.journal_id)
        uuid(self.generation)
        integer(self.sequence)
        digest(self.prefix_digest)

    @classmethod
    def from_dict(cls, value):
        object_fields(value, "version journal_id generation sequence prefix_digest")
        return cls(**value)

    def to_dict(self):
        return dict(vars(self))

    def same_history(self, other):
        return (self.journal_id, self.generation) == (other.journal_id, other.generation)


def author(value):
    object_fields(value, "credential_id subject_id")
    uuid(value["credential_id"])
    identifier(value["subject_id"])


@dataclass(frozen=True)
class ConsumerCheckpoint:
    revision: int
    cursor: VerifiedCursor
    checkpoint_digest: str


def checkpoint(value, consumer_id, subject_id):
    object_fields(
        value,
        "schema_version consumer_id revision cursor author updated_at_millis checkpoint_digest",
    )
    require(type(value["schema_version"]) is int and value["schema_version"] == 2)
    require(value["consumer_id"] == consumer_id)
    author(value["author"])
    require(value["author"]["subject_id"] == subject_id)
    integer(value["updated_at_millis"], -(2**63), 2**63 - 1)
    return ConsumerCheckpoint(
        integer(value["revision"], 1),
        VerifiedCursor.from_dict(value["cursor"]),
        digest(value["checkpoint_digest"]),
    )


def envelope(value, field, expected_scope=None):
    fields = "contract_version " + field + (" scope" if expected_scope is not None else "")
    object_fields(value, fields)
    require(type(value["contract_version"]) is int and value["contract_version"] == 2)
    if expected_scope is not None:
        require(scope(value["scope"]) == expected_scope, "response_scope_mismatch")
    return value[field]


def diagnosis(value, expected_scope, consumer_id, subject_id, witness):
    d = envelope(value, "diagnostics", expected_scope)
    object_fields(
        d,
        "consumer_id active baseline high_watermark checkpoint checkpoint_status pending_positions witness_status checkpoint_relative_to_witness",
    )
    require(d["consumer_id"] == consumer_id and type(d["active"]) is bool)
    require(d["checkpoint_status"] in ("missing", "compatible", "history_incompatible"))
    require(d["witness_status"] in ("not_provided", "compatible", "history_incompatible"))
    require((d["witness_status"] == "not_provided") == (witness is None))
    cp = None if d["checkpoint"] is None else checkpoint(d["checkpoint"], consumer_id, subject_id)
    require((cp is None) == (d["checkpoint_status"] == "missing"))
    if d["active"]:
        baseline = VerifiedCursor.from_dict(d["baseline"])
        tip = VerifiedCursor.from_dict(d["high_watermark"])
        require(baseline.same_history(tip) and baseline.sequence <= tip.sequence)
        if baseline.sequence == tip.sequence:
            require(baseline == tip)
    else:
        require(d["baseline"] is None and d["high_watermark"] is None)
        require(d["checkpoint_status"] != "compatible" and d["witness_status"] != "compatible")
    if d["checkpoint_status"] == "compatible":
        require(
            cp.cursor.same_history(tip) and baseline.sequence <= cp.cursor.sequence <= tip.sequence
        )
        require(integer(d["pending_positions"]) == tip.sequence - cp.cursor.sequence)
        if cp.cursor.sequence == tip.sequence:
            require(cp.cursor == tip)
        if cp.cursor.sequence == baseline.sequence:
            require(cp.cursor == baseline)
    else:
        require(d["pending_positions"] is None)
    if d["witness_status"] == "compatible":
        require(witness.same_history(tip) and baseline.sequence <= witness.sequence <= tip.sequence)
        if witness.sequence == tip.sequence:
            require(witness == tip)
        if witness.sequence == baseline.sequence:
            require(witness == baseline)
    if d["checkpoint_status"] == d["witness_status"] == "compatible":
        order = (
            "before"
            if cp.cursor.sequence < witness.sequence
            else "after" if cp.cursor.sequence > witness.sequence else "equal"
        )
        require(d["checkpoint_relative_to_witness"] == order)
        if order == "equal":
            require(cp.cursor == witness)
    else:
        require(d["checkpoint_relative_to_witness"] is None)
    return d, cp


def page(value, expected_scope, after, through, limit):
    p = envelope(value, "page", expected_scope)
    object_fields(p, "active baseline changes next_cursor high_watermark complete")
    require(p["active"] is True and type(p["complete"]) is bool)
    baseline, end, fence = [
        VerifiedCursor.from_dict(p[k]) for k in ("baseline", "next_cursor", "high_watermark")
    ]
    require(all(after.same_history(c) for c in (baseline, end, fence)))
    require(baseline.sequence <= after.sequence <= end.sequence <= fence.sequence)
    if baseline.sequence == after.sequence:
        require(baseline == after)
    if end.sequence == fence.sequence:
        require(end == fence)
    if through is not None:
        require(fence == through)
    require(isinstance(p["changes"], list) and len(p["changes"]) <= limit)
    previous = after
    for row in p["changes"]:
        object_fields(row, "cursor change")
        cursor = VerifiedCursor.from_dict(row["cursor"])
        require(cursor.same_history(after) and cursor.sequence == previous.sequence + 1)
        change = object_fields(
            row["change"],
            "schema_version cursor kind record_id record_revision author committed_at_millis change_digest",
        )
        require(type(change["schema_version"]) is int and change["schema_version"] == 1)
        require(
            change["kind"]
            in ("created", "derived", "updated", "deleted", "embedding_attached", "reviewed")
        )
        object_fields(change["cursor"], "journal_id sequence")
        require(change["cursor"] == {"journal_id": cursor.journal_id, "sequence": cursor.sequence})
        integer(change["cursor"]["sequence"])
        uuid(change["record_id"])
        integer(change["record_revision"], 1)
        author(change["author"])
        integer(change["committed_at_millis"], -(2**63), 2**63 - 1)
        digest(change["change_digest"])
        previous = cursor
    require(previous == end)
    require(p["complete"] == (end == fence))
    require(p["complete"] or bool(p["changes"]))
    return p, end, fence
