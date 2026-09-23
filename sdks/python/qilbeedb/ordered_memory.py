"""Bounded, scoped chronological reads using the standard-library transport."""
from copy import deepcopy
from . import _consumer_wire as wire
from .consumer import VerifiedMemoryConsumer


def _cursor(value):
    wire.require(isinstance(value, str) and 0 < len(value) <= 1024,
                 "invalid_ordered_cursor")
    wire.require(len(value) % 2 == 0 and all(c in "0123456789abcdef" for c in value),
                 "invalid_ordered_cursor")


class OrderedMemoryClient:
    """Read one live chronological page in an explicitly authorized scope.

    The server owns ordering, eligibility and cursor binding. Each call verifies
    the configured company and subject with the credential used for that call.
    No consumer registration, checkpoint, cache, retry or fallback is created.
    """

    def __init__(self, base_url, api_key, *, tenant_id, subject_id, scope,
                 timeout=10.0, max_response_bytes=16 * 1024 * 1024):
        self._identity = VerifiedMemoryConsumer(
            base_url, api_key, tenant_id=tenant_id, subject_id=subject_id,
            scope=scope, consumer_id="ordered-memory-reader", timeout=timeout,
            max_response_bytes=max_response_bytes,
        )

    def query(self, *, order="created_desc", limit=40, scan_limit=1000,
              cursor=None, text_contains=None, tag=None, episode_type=None):
        """Return the full response, preserving continuation and work metadata.

        Empty records with a continuation cursor are a partial page, not an empty
        scope. Keep the order and filters unchanged when continuing; use no cursor
        to restart. Pages are live observations, never a point-in-time snapshot.
        """
        wire.require(order in ("created_desc", "created_asc"), "invalid_date_order")
        wire.integer(limit, 1, 100)
        wire.integer(scan_limit, 1, 10000)
        if cursor is not None:
            _cursor(cursor)
        for value in (text_contains, tag, episode_type):
            wire.require(value is None or isinstance(value, str), "invalid_query_filter")
        query = dict(order=order, limit=limit, scan_limit=scan_limit, cursor=cursor,
                     text_contains=text_contains, tag=tag, episode_type=episode_type)
        body = dict(contract_version=1, scope=deepcopy(self._identity._scope), query=query)
        with self._identity._operation() as token:
            response = self._identity._http.request(token, "/api/v1/memory/query/ordered", body)
        self._validate(response, query)
        return response

    def _validate(self, response, query):
        wire.object_fields(response, "contract_version scope page")
        wire.require(type(response["contract_version"]) is int and response["contract_version"] == 1)
        wire.require(response["scope"] == self._identity._scope, "ordered_scope_mismatch")
        page = response["page"]
        wire.object_fields(page, "records next_cursor stop_reason evaluated_at_millis scanned_records record_bytes dependency_work")
        wire.integer(page["evaluated_at_millis"], -(2**63), 2**63 - 1)
        wire.integer(page["scanned_records"], 0, query["scan_limit"])
        wire.integer(page["record_bytes"], 0, 8 * 1024 * 1024)
        wire.object_fields(page["dependency_work"], "records_examined bytes_examined")
        for count in page["dependency_work"].values():
            wire.integer(count)
        records = page["records"]
        wire.require(isinstance(records, list) and len(records) <= query["limit"])
        wire.require(len(records) <= page["scanned_records"])
        keys, ids = [], set()
        for record in records:
            wire.require(isinstance(record, dict))
            wire.require(type(record.get("schema_version")) is int and record["schema_version"] == 1)
            rid = wire.uuid(record.get("record_id"))
            wire.require(rid not in ids, "duplicate_ordered_record")
            ids.add(rid)
            wire.integer(record.get("revision"), 1)
            created = wire.integer(record.get("created_at_millis"), -(2**63), 2**63 - 1)
            wire.integer(record.get("modified_at_millis"), -(2**63), 2**63 - 1)
            wire.author(record.get("author"))
            wire.require(isinstance(record.get("payload"), dict), "deleted_ordered_record")
            keys.append((created, rid))
        wire.require(keys == sorted(keys, reverse=query["order"] == "created_desc"),
                     "unordered_memory_page")
        stop, cursor = page["stop_reason"], page["next_cursor"]
        wire.require(stop in ("exhausted", "record_limit", "scan_limit", "byte_limit"))
        wire.require((cursor is None) == (stop == "exhausted"), "invalid_ordered_continuation")
        if cursor is not None:
            _cursor(cursor)
            wire.require(cursor != query["cursor"], "ordered_cursor_did_not_advance")
