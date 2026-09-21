"""Verified memory consumption with caller-owned durable, idempotent effects.

This module uses only the Python standard library. It never generates embeddings,
activates journals, initializes missing progress implicitly, or recovers history.
"""

from contextlib import contextmanager
from copy import deepcopy
from dataclasses import dataclass
from hashlib import sha256
import inspect
import json
from threading import Lock
from typing import Any, Callable, Dict, Mapping, Optional, Protocol, Union

from ._consumer_http import ConsumerHTTP
from . import _consumer_wire as wire
from ._consumer_wire import (
    ConsumerAPIError,
    ConsumerCheckpoint,
    ConsumerError,
    ConsumerProtocolError,
    ConsumerTransportError,
    ReconciliationRequired,
    VerifiedCursor,
)


@dataclass(frozen=True)
class MemoryDelivery:
    """An event identity and metadata; refetch current memory before materializing it."""

    binding_id: str
    delivery_id: str
    cursor: VerifiedCursor
    change: Mapping[str, Any]


class MemoryChangeSink(Protocol):
    """Own effects, deduplication and their contiguous witness in one durable boundary.

    Coordinate writers to a binding. A witness must never advance before its
    effects are durable or move backwards during ordinary consumption.
    """

    def load_witness(self, binding_id: str) -> Optional[VerifiedCursor]: ...

    def apply(self, delivery: MemoryDelivery) -> None: ...


@dataclass(frozen=True)
class ConsumptionResult:
    delivered: int
    next_cursor: VerifiedCursor
    high_watermark: VerifiedCursor
    complete: bool
    checkpoint: ConsumerCheckpoint
    # The receipt is historical; checkpoint is obtained by a fresh server read.
    receipt: Optional[Mapping[str, Any]]


def _hash(value):
    return sha256(
        json.dumps(
            value, sort_keys=True, ensure_ascii=True, separators=(",", ":"), allow_nan=False
        ).encode("ascii")
    ).hexdigest()


class VerifiedMemoryConsumer:
    """Consume one bounded v2 page, then acknowledge with exact checkpoint CAS.

    Requires the consumer-diagnostics feature in the 0.10.0 server preview.
    Configure the expected tenant and subject explicitly. Each operation pins one
    credential and verifies that identity before accessing the selected scope.
    ``sink`` remains the authority for effects, not a local cache of server state.
    """

    def __init__(
        self,
        base_url: str,
        api_key: Union[str, Callable[[], str]],
        *,
        tenant_id: str,
        subject_id: str,
        scope: Mapping[str, Any],
        consumer_id: str,
        page_size: int = 100,
        timeout: float = 10.0,
        max_response_bytes: int = 4 * 1024 * 1024,
    ):
        wire.identifier(tenant_id)
        wire.identifier(subject_id)
        wire.identifier(consumer_id, 128)
        if type(page_size) is not int or not 1 <= page_size <= 256:
            raise ValueError("page_size must be between 1 and 256")
        if not isinstance(api_key, str) and not callable(api_key):
            raise TypeError("api_key must be a credential or a credential supplier")
        self._http = ConsumerHTTP(base_url, timeout, max_response_bytes)
        self._credential = api_key
        self._tenant = tenant_id
        self._subject = subject_id
        self._scope = wire.scope(dict(scope))
        self._consumer = consumer_id
        self._limit = page_size
        self._lock = Lock()
        self._binding_id = _hash(
            ["qilbee.consumer.binding.v1", tenant_id, subject_id, self._scope, consumer_id]
        )

    @property
    def binding_id(self) -> str:
        """Stable sink namespace across credential rotation and server relocation."""
        return self._binding_id

    @contextmanager
    def _operation(self):
        if not self._lock.acquire(blocking=False):
            raise ConsumerError("consumer_already_running")
        try:
            token = self._credential() if callable(self._credential) else self._credential
            if (
                not isinstance(token, str)
                or not token
                or not token.isascii()
                or any(c.isspace() or ord(c) < 32 or ord(c) == 127 for c in token)
            ):
                raise ConsumerError("invalid_credential")
            identity = self._http.request(token, "/api/v1/identity")
            wire.object_fields(identity, "contract_version credential")
            wire.require(
                type(identity["contract_version"]) is int and identity["contract_version"] == 1
            )
            credential = identity["credential"]
            wire.require(isinstance(credential, dict) and isinstance(credential.get("spec"), dict))
            if (
                credential.get("tenant_id") != self._tenant
                or credential["spec"].get("subject_id") != self._subject
            ):
                raise ReconciliationRequired("credential_identity_mismatch")
            wire.require(credential.get("revoked_at_millis") is None)
            yield token
        finally:
            self._lock.release()

    def _witness(self, sink):
        try:
            witness = sink.load_witness(self.binding_id)
        except Exception:
            raise ConsumerError("sink_witness_read_failed") from None
        if not isinstance(witness, VerifiedCursor):
            raise ReconciliationRequired("durable_witness_required")
        return witness

    def _diagnose(self, token, witness):
        body = {
            "contract_version": 2,
            "scope": self._scope,
            "consumer_id": self._consumer,
            "witness": None if witness is None else witness.to_dict(),
        }
        response = self._http.request(token, "/api/v2/memory/consumers/diagnose", body)
        return wire.diagnosis(response, self._scope, self._consumer, self._subject, witness)

    @staticmethod
    def _compatible(diagnosis, checkpoint, witness, *, allow_missing=False):
        if (
            diagnosis["witness_status"] != "compatible"
            or diagnosis["checkpoint_status"] == "history_incompatible"
        ):
            raise ReconciliationRequired("history_reconciliation_required")
        if checkpoint is None:
            if allow_missing:
                return
            raise ReconciliationRequired("checkpoint_initialization_required")
        if checkpoint.cursor.sequence > witness.sequence:
            raise ReconciliationRequired("checkpoint_ahead_of_durable_effects")

    def diagnose(self, witness: Optional[VerifiedCursor] = None) -> Dict[str, Any]:
        """Read bounded scoped observations without changing any progress."""
        if witness is not None and not isinstance(witness, VerifiedCursor):
            raise TypeError("witness must be a complete VerifiedCursor")
        with self._operation() as token:
            return self._diagnose(token, witness)[0]

    def _commit(self, token, expected, target):
        command = {
            "contract_version": 2,
            "consumer_id": self._consumer,
            "expected_revision": 0 if expected is None else expected.revision,
            "expected_checkpoint_digest": None if expected is None else expected.checkpoint_digest,
            "cursor": target.to_dict(),
        }
        command["idempotency_key"] = "consumer-v1-" + _hash([self.binding_id, command])
        response = self._http.request(
            token,
            "/api/v2/memory/checkpoints",
            {"scope": self._scope, "command": command},
            mutation=True,
        )
        try:
            receipt = wire.envelope(response, "receipt")
            wire.object_fields(
                receipt, "contract_version idempotency_key checkpoint receipt_digest"
            )
            wire.require(
                type(receipt["contract_version"]) is int and receipt["contract_version"] == 2
            )
            wire.require(receipt["idempotency_key"] == command["idempotency_key"])
            wire.digest(receipt["receipt_digest"])
            saved = wire.checkpoint(receipt["checkpoint"], self._consumer, self._subject)
            wire.require(
                saved.revision == command["expected_revision"] + 1 and saved.cursor == target
            )
            return receipt, saved
        except ConsumerError as error:
            error.checkpoint_outcome = "unknown"
            raise

    def _current_after_commit(self, token, sink, saved):
        try:
            witness = self._witness(sink)
            diagnosis, current = self._diagnose(token, witness)
            self._compatible(diagnosis, current, witness)
            if current.revision < saved.revision or (
                current.revision == saved.revision and current != saved
            ):
                raise ReconciliationRequired("checkpoint_changed_after_acknowledgement")
            return current
        except ConsumerError as error:
            error.checkpoint_outcome = "acknowledged"
            raise
        except Exception:
            raise ConsumerError(
                "sink_witness_read_failed", checkpoint_outcome="acknowledged"
            ) from None

    def initialize(self, sink: MemoryChangeSink, cursor: VerifiedCursor) -> ConsumerCheckpoint:
        """Create missing progress only after explicit external reconciliation.

        The sink must already durably retain this exact cursor for this binding.
        Existing progress is never overwritten; use consume_once to resume it.
        """
        if not isinstance(cursor, VerifiedCursor):
            raise TypeError("cursor must be a complete VerifiedCursor")
        with self._operation() as token:
            witness = self._witness(sink)
            if witness != cursor:
                raise ReconciliationRequired("initial_witness_mismatch")
            diagnosis, current = self._diagnose(token, witness)
            self._compatible(diagnosis, current, witness, allow_missing=True)
            if current is not None:
                raise ConsumerError("checkpoint_already_initialized")
            _, saved = self._commit(token, None, cursor)
            return self._current_after_commit(token, sink, saved)

    def consume_once(
        self, sink: MemoryChangeSink, *, through: Optional[VerifiedCursor] = None
    ) -> ConsumptionResult:
        """Apply one fully validated page and CAS-acknowledge only durable effects.

        A sink witness ahead of the server checkpoint is resumable: read after
        that witness, which the server verifies in the same snapshot as the page.
        Pass high_watermark as through on following calls to retain a fixed fence.
        On any failure, stop; there is no hidden retry, rewind or checkpoint reset.
        """
        if through is not None and not isinstance(through, VerifiedCursor):
            raise TypeError("through must be a complete VerifiedCursor")
        with self._operation() as token:
            witness = self._witness(sink)
            diagnosis, current = self._diagnose(token, witness)
            self._compatible(diagnosis, current, witness)
            if through is not None and (
                not witness.same_history(through) or witness.sequence > through.sequence
            ):
                raise ReconciliationRequired("fence_incompatible_with_durable_effects")
            query = {
                "after": witness.to_dict(),
                "through": None if through is None else through.to_dict(),
                "limit": self._limit,
            }
            response = self._http.request(
                token,
                "/api/v2/memory/changes",
                {"contract_version": 2, "scope": self._scope, "query": query},
            )
            page, target, fence = wire.page(response, self._scope, witness, through, self._limit)
            for row in page["changes"]:
                cursor = VerifiedCursor.from_dict(row["cursor"])
                delivery = MemoryDelivery(
                    self.binding_id,
                    _hash(["qilbee.consumer.delivery.v1", self.binding_id, cursor.to_dict()]),
                    cursor,
                    deepcopy(row["change"]),
                )
                try:
                    returned = sink.apply(delivery)
                    if inspect.isawaitable(returned):
                        if inspect.iscoroutine(returned):
                            returned.close()
                        raise ConsumerError("synchronous_sink_required")
                    if self._witness(sink) != cursor:
                        raise ReconciliationRequired("sink_did_not_retain_delivery")
                except ConsumerError:
                    raise
                except Exception:
                    raise ConsumerError("sink_apply_failed") from None
            # Also validate an empty-page witness and prevent an unrelated sink writer's rewind.
            if self._witness(sink) != target:
                raise ReconciliationRequired("sink_witness_changed")
            receipt = None
            if current.cursor != target:
                receipt, saved = self._commit(token, current, target)
                current = self._current_after_commit(token, sink, saved)
            return ConsumptionResult(
                len(page["changes"]), target, fence, page["complete"], current, receipt
            )
