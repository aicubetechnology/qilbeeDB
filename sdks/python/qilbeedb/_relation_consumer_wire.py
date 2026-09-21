"""Closed relation-stream v1 contracts, separate from both memory cursor formats."""

from dataclasses import dataclass
from ._consumer_wire import (
    require,
    object_fields,
    integer,
    identifier,
    digest,
    uuid,
    scope,
    author,
)


@dataclass(frozen=True)
class RelationCursor:
    version: int
    stream: str
    journal_id: str
    sequence: int
    prefix_digest: str

    def __post_init__(self):
        require(type(self.version) is int and self.version == 1)
        require(self.stream == "typed_memory_relations")
        uuid(self.journal_id)
        integer(self.sequence)
        digest(self.prefix_digest)

    @classmethod
    def from_dict(cls, value):
        object_fields(value, "version stream journal_id sequence prefix_digest")
        return cls(**value)

    def to_dict(self):
        return dict(vars(self))

    def same_history(self, other):
        return isinstance(other, RelationCursor) and self.journal_id == other.journal_id


@dataclass(frozen=True)
class RelationCheckpoint:
    revision: int
    cursor: RelationCursor
    checkpoint_digest: str


def checkpoint(value, consumer_id, subject_id):
    object_fields(
        value,
        "schema_version consumer_id revision cursor author updated_at_millis checkpoint_digest",
    )
    require(type(value["schema_version"]) is int and value["schema_version"] == 1)
    require(value["consumer_id"] == consumer_id)
    author(value["author"])
    require(value["author"]["subject_id"] == subject_id)
    integer(value["updated_at_millis"], -(2**63), 2**63 - 1)
    return RelationCheckpoint(
        integer(value["revision"], 1),
        RelationCursor.from_dict(value["cursor"]),
        digest(value["checkpoint_digest"]),
    )


def envelope(value, field, expected_scope=None):
    object_fields(
        value, "contract_version " + field + (" scope" if expected_scope is not None else "")
    )
    require(type(value["contract_version"]) is int and value["contract_version"] == 1)
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
        baseline = RelationCursor.from_dict(d["baseline"])
        tip = RelationCursor.from_dict(d["high_watermark"])
        require(baseline.same_history(tip) and baseline.sequence == 0)
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
        RelationCursor.from_dict(p[k]) for k in ("baseline", "next_cursor", "high_watermark")
    ]
    require(all(after.same_history(c) for c in (baseline, end, fence)))
    require(baseline.sequence == 0 and after.sequence <= end.sequence <= fence.sequence)
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
        cursor = RelationCursor.from_dict(row["cursor"])
        require(cursor.same_history(after) and cursor.sequence == previous.sequence + 1)
        change = object_fields(
            row["change"],
            "schema_version kind relation_id relation_revision source target relation_kind author committed_at_millis relation_digest receipt_digest",
        )
        require(type(change["schema_version"]) is int and change["schema_version"] == 1)
        require(change["kind"] in ("asserted", "retired", "restored", "reviewed"))
        require(
            change["relation_kind"]
            in (
                "semantic_related",
                "same_entity",
                "temporal_before",
                "causal_claim",
                "supports",
                "contradicts",
            )
        )
        uuid(change["relation_id"])
        integer(change["relation_revision"], 1)
        for endpoint in (change["source"], change["target"]):
            object_fields(endpoint, "record_id revision")
            uuid(endpoint["record_id"])
            integer(endpoint["revision"], 1)
        require(change["source"]["record_id"] != change["target"]["record_id"])
        author(change["author"])
        integer(change["committed_at_millis"], -(2**63), 2**63 - 1)
        digest(change["relation_digest"])
        digest(change["receipt_digest"])
        previous = cursor
    require(previous == end)
    require(p["complete"] == (end == fence))
    require(p["complete"] or bool(p["changes"]))
    return p, end, fence
