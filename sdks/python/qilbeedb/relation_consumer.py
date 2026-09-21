"""Verified, bounded typed-relation consumption without model or storage dependencies."""

from dataclasses import dataclass
from typing import Any, Mapping, Optional, Protocol

from . import _relation_consumer_wire as wire
from ._relation_consumer_wire import RelationCheckpoint, RelationCursor
from .consumer import VerifiedMemoryConsumer, _hash


@dataclass(frozen=True)
class RelationDelivery:
    """Historical assertion metadata; it does not establish current eligibility."""

    binding_id: str
    delivery_id: str
    cursor: RelationCursor
    change: Mapping[str, Any]


class RelationChangeSink(Protocol):
    """Commit effects, deduplication and the complete witness in one durable boundary."""

    def load_witness(self, binding_id: str) -> Optional[RelationCursor]: ...

    def apply(self, delivery: RelationDelivery) -> None: ...


@dataclass(frozen=True)
class RelationConsumptionResult:
    delivered: int
    next_cursor: RelationCursor
    high_watermark: RelationCursor
    complete: bool
    checkpoint: RelationCheckpoint
    receipt: Optional[Mapping[str, Any]]


class VerifiedRelationConsumer(VerifiedMemoryConsumer):
    """Consume the server 0.13.0 relation feed with an explicit durable sink witness.

    Memory and relation bindings, delivery identities, cursors, and checkpoint
    contracts remain distinct, even with the same configured consumer name.
    Endpoint changes and expiry require separate current-state invalidation.
    This client never activates a journal or reconciles history automatically.
    """

    _wire = wire
    _cursor_type = RelationCursor
    _delivery_type = RelationDelivery
    _result_type = RelationConsumptionResult
    _protocol_version = 1
    _route = "/api/v1/memory/relations"
    _binding_domain = "qilbee.relation-consumer.binding.v1"
    _delivery_domain = "qilbee.relation-consumer.delivery.v1"
    _command_prefix = "relation-consumer-v1-"

    def _command(self, expected, target):
        command = {
            "contract_version": 1,
            "consumer_id": self._consumer,
            "expected_revision": 0 if expected is None else expected.revision,
            "expected_checkpoint_digest": None if expected is None else expected.checkpoint_digest,
            "operation": {"type": "advance", "cursor": target.to_dict()},
        }
        command["idempotency_key"] = self._command_prefix + _hash([self.binding_id, command])
        return command

    def _validate_receipt(self, response, command, expected, target):
        receipt = wire.envelope(response, "receipt")
        wire.object_fields(
            receipt,
            "contract_version idempotency_key previous checkpoint reconciliation_evidence receipt_digest",
        )
        wire.require(type(receipt["contract_version"]) is int and receipt["contract_version"] == 1)
        wire.require(receipt["idempotency_key"] == command["idempotency_key"])
        wire.require(receipt["reconciliation_evidence"] is None)
        wire.digest(receipt["receipt_digest"])
        previous = (
            None
            if receipt["previous"] is None
            else wire.checkpoint(receipt["previous"], self._consumer, self._subject)
        )
        wire.require(previous == expected)
        saved = wire.checkpoint(receipt["checkpoint"], self._consumer, self._subject)
        wire.require(saved.revision == command["expected_revision"] + 1 and saved.cursor == target)
        return receipt, saved

    def diagnose(self, witness: Optional[RelationCursor] = None):
        """Observe current scoped history and progress without changing either."""
        return super().diagnose(witness)

    def initialize(self, sink: RelationChangeSink, cursor: RelationCursor) -> RelationCheckpoint:
        """Initialize missing progress after the sink durably reconciles this exact cursor."""
        return super().initialize(sink, cursor)

    def consume_once(
        self, sink: RelationChangeSink, *, through: Optional[RelationCursor] = None
    ) -> RelationConsumptionResult:
        """Validate a whole bounded page, apply durable effects, then CAS progress.

        Retain high_watermark as through for a finite catch-up cycle. Failures stop
        the call without automatic retries, checkpoint reset, or witness rewind.
        A valid historical receipt is followed by a fresh current-state diagnosis.
        """
        return super().consume_once(sink, through=through)
