"""External graph consolidation with durable intent and no implicit model retries.

Only the standard library is required. Provider credentials stay in the supplied
extractor. SQLiteConsolidationJournal supports local POSIX filesystems; use one
journal across processes sharing an attempt. It is not a distributed lock.
"""

from contextlib import contextmanager
from copy import deepcopy
from dataclasses import dataclass
import json
import os
from pathlib import Path
import sqlite3
from uuid import uuid4

from . import _consumer_wire as wire
from . import _consolidation_wire as context_wire
from .consumer import VerifiedMemoryConsumer, _hash


class ConsolidationStopped(Exception):
    """Execution stopped; this does not imply zero provider consumption."""

    def __init__(self, code):
        self.code = code
        super().__init__("Consolidation stopped: " + code)


def _copy(value, maximum=65536):
    raw = json.dumps(value, allow_nan=False, separators=(",", ":"))
    wire.require(len(raw.encode("utf-8")) <= maximum, "consolidation_value_too_large")
    return json.loads(raw)


def _usage(value):
    wire.require(isinstance(value, dict))
    if value.get("status") == "unknown":
        wire.object_fields(value, "status")
    else:
        value = dict(value)
        value.setdefault("cost_microusd", None)
        wire.object_fields(value, "status model_calls input_tokens output_tokens cost_microusd")
        wire.require(value["status"] == "reported")
        for key in ("model_calls", "input_tokens", "output_tokens"):
            wire.integer(value[key])
        if value["cost_microusd"] is not None:
            wire.integer(value["cost_microusd"])
    return value


class ConsolidationClient:
    """Authenticated owner-scoped lifecycle. Commands never retry automatically.

    Keep idempotency keys and complete commands durable before sending mutations.
    A replayed receipt describes history: inspect before starting external work.
    """

    def __init__(self, base_url, api_key, *, tenant_id, subject_id, scope, timeout=10.0):
        self._identity = VerifiedMemoryConsumer(
            base_url, api_key, tenant_id=tenant_id, subject_id=subject_id,
            scope=scope, consumer_id="consolidation", timeout=timeout,
            max_response_bytes=16 * 1024 * 1024,
        )
        self.binding_id = _hash([
            "qilbee.consolidation.client.v1", base_url.rstrip("/"), tenant_id,
            subject_id, self._identity._scope,
        ])

    def _request(self, route, fields, field, mutation=False):
        body = dict(fields, contract_version=1, scope=self._identity._scope)
        with self._identity._operation() as token:
            result = self._identity._http.request(
                token, "/api/v1/memory/consolidation/" + route, body, mutation=mutation
            )
        wire.object_fields(result, "contract_version " + field + ("" if mutation else " scope"))
        wire.require(type(result["contract_version"]) is int and result["contract_version"] == 1)
        if not mutation:
            wire.require(wire.scope(result["scope"]) == self._identity._scope)
        wire.require(isinstance(result[field], dict))
        return result[field]

    def command(self, operation, *, idempotency_key):
        wire.identifier(idempotency_key)
        operation = _copy(operation)
        receipt = self._request(
            "commands", {"idempotency_key": idempotency_key, "operation": operation},
            "receipt", True,
        )
        self._receipt(receipt)
        wire.require(receipt["idempotency_key"] == idempotency_key)
        if "job_id" in operation:
            wire.require(receipt["job_id"] == operation["job_id"])
            wire.require(receipt["revision"] == operation["expected_revision"] + 1)
        actions = {"create": "created", "claim": "claimed", "renew": "renewed",
                   "publish": "published", "fail": "failed", "cancel": "cancelled",
                   "recover_expired": "recovered", "reconcile_usage": "usage_reconciled"}
        wire.require(receipt["action"] == actions.get(operation.get("type")))
        wire.author(receipt["author"])
        # Native company cancellation can legitimately be replayed by the owner.
        if receipt["action"] != "cancelled":
            wire.require(receipt["author"]["subject_id"] == self._identity._subject)
        return receipt

    def _receipt(self, receipt):
        wire.object_fields(receipt, "contract_version idempotency_key job_id revision action author committed_at_millis job_digest command_digest receipt_digest")
        wire.integer(receipt["contract_version"], 1, 1)
        wire.identifier(receipt["idempotency_key"])
        wire.uuid(receipt["job_id"])
        wire.integer(receipt["revision"], 1)
        wire.integer(receipt["committed_at_millis"], -(2**63), 2**63 - 1)
        wire.author(receipt["author"])
        wire.require(receipt["action"] in ("created", "claimed", "renewed", "published", "failed", "cancelled", "recovered", "usage_reconciled"))
        for name in ("job_digest", "command_digest", "receipt_digest"):
            wire.digest(receipt[name])
        # Digests are server-verified history identities, not client attestations.
        return receipt

    def inspect(self, job_id):
        wire.uuid(job_id)
        result = self._request("inspect", {"job_id": job_id}, "inspection")
        wire.object_fields(result, "job evaluated_at_millis lease_active recoverable source_failure dependency_work")
        wire.require(type(result["lease_active"]) is bool and type(result["recoverable"]) is bool)
        self._job(result["job"], job_id)
        context_wire.source_failure(result["source_failure"])
        context_wire.dependency_work(result["dependency_work"])
        wire.integer(result["evaluated_at_millis"], -(2**63), 2**63 - 1)
        wire.require(not result["lease_active"] or result["job"]["status"] == "running")
        wire.require(result["recoverable"] == (result["job"]["status"] == "running" and not result["lease_active"]))
        return result

    def _job(self, job, job_id):
        wire.object_fields(job, "schema_version job_id revision created_by created_at_millis modified_at_millis spec status attempts output_receipts")
        wire.require(type(job["schema_version"]) is int and job["schema_version"] == 1)
        wire.require(job["job_id"] == job_id)
        wire.integer(job["revision"], 1)
        wire.author(job["created_by"])
        wire.require(job["created_by"]["subject_id"] == self._identity._subject)
        wire.require(job["status"] in ("ready", "running", "published", "cancelled", "exhausted"))
        wire.require(isinstance(job["attempts"], list) and len(job["attempts"]) <= 32)
        for number, attempt in enumerate(job["attempts"], 1):
            wire.object_fields(attempt, "number worker_id credential_id fence storage_incarnation claimed_at_millis expires_at_millis ended_at_millis outcome usage evidence_ref usage_evidence_ref")
            wire.require(type(attempt["number"]) is int and attempt["number"] == number)
            for key in ("credential_id", "fence", "storage_incarnation"):
                wire.uuid(attempt[key])
            _usage(attempt["usage"])
        spec = job["spec"]
        wire.require(isinstance(spec, dict) and isinstance(spec.get("sources"), list))
        wire.require(2 <= len(spec["sources"]) <= 16)
        ids = set()
        for source in spec["sources"]:
            wire.object_fields(source, "record_id revision")
            wire.uuid(source["record_id"])
            wire.integer(source["revision"], 1)
            ids.add(source["record_id"])
        wire.require(len(ids) == len(spec["sources"]))
        wire.integer(spec.get("max_relations"), 1, 16)
        wire.integer(spec.get("max_attempts"), 1, 32)
        wire.integer(spec.get("lease_millis"), 1000, 900000)
        wire.integer(spec.get("max_attempt_millis"), spec["lease_millis"], 86400000)
        wire.require(len(job["attempts"]) <= spec["max_attempts"])
        context_wire.job_details(job)
        return job

    def context(self, job_id, expected_revision, fence):
        wire.uuid(job_id)
        wire.integer(expected_revision, 1)
        wire.uuid(fence)
        result = self._request("context", {"job_id": job_id, "expected_revision": expected_revision, "fence": fence}, "context")
        wire.object_fields(result, "job_id job_revision fence records evaluated_at_millis record_bytes dependency_work")
        wire.integer(result["job_revision"], 1)
        wire.require(result["job_id"] == job_id and result["job_revision"] == expected_revision and result["fence"] == fence)
        wire.require(isinstance(result["records"], list) and 2 <= len(result["records"]) <= 16)
        wire.integer(result["record_bytes"], 1, 8 * 1024 * 1024)
        context_wire.timestamp(result["evaluated_at_millis"])
        context_wire.dependency_work(result["dependency_work"])
        seen = set()
        for record in result["records"]:
            context_wire.context_record(record, result["evaluated_at_millis"])
            wire.require(record["record_id"] not in seen, "duplicate_context_record")
            seen.add(record["record_id"])
        return result

    def query(self, *, limit=25, scan_limit=100, after=None, status=None):
        wire.integer(limit, 1, 100)
        wire.integer(scan_limit, 1, 1000)
        statuses = ("ready", "running", "published", "cancelled", "exhausted")
        wire.require(status is None or status in statuses)
        if after is not None:
            wire.uuid(after)
        page = self._request("query", {"query": {"limit": limit, "scan_limit": scan_limit, "after": after, "status": status}}, "page")
        wire.object_fields(page, "jobs next_after records_examined record_bytes evaluated_at_millis")
        wire.require(isinstance(page["jobs"], list) and len(page["jobs"]) <= limit)
        wire.integer(page["records_examined"], len(page["jobs"]), scan_limit)
        wire.integer(page["record_bytes"], 0, 4 * 1024 * 1024)
        wire.integer(page["evaluated_at_millis"], -(2**63), 2**63 - 1)
        previous = after
        for job in page["jobs"]:
            wire.object_fields(job, "job_id revision status created_at_millis modified_at_millis attempts lease_active recoverable")
            wire.uuid(job["job_id"])
            wire.require(previous is None or job["job_id"] > previous, "non_advancing_job_page")
            previous = job["job_id"]
            wire.integer(job["revision"], 1)
            wire.integer(job["attempts"], 0, 32)
            wire.require(job["status"] in statuses and (status is None or job["status"] == status))
            for key in ("created_at_millis", "modified_at_millis"):
                wire.integer(job[key], -(2**63), 2**63 - 1)
            wire.require(type(job["lease_active"]) is bool and type(job["recoverable"]) is bool)
            wire.require(not job["lease_active"] or job["status"] == "running")
            wire.require(job["recoverable"] == (job["status"] == "running" and not job["lease_active"]))
        cursor = page["next_after"]
        if cursor is not None:
            wire.uuid(cursor)
            wire.require(page["records_examined"] > 0)
            wire.require(after is None or cursor > after, "non_advancing_job_page")
            wire.require(previous is None or cursor >= previous, "cursor_precedes_job")
        return page

    def revision(self, job_id, revision):
        wire.uuid(job_id)
        wire.integer(revision, 1)
        history = self._request("revision", {"job_id": job_id, "revision": revision}, "history")
        wire.object_fields(history, "job receipt")
        job = self._job(history["job"], job_id)
        receipt = self._receipt(history["receipt"])
        wire.require(job["revision"] == revision and receipt["revision"] == revision)
        wire.require(receipt["job_id"] == job_id)
        wire.require(receipt["committed_at_millis"] == job["modified_at_millis"])
        if receipt["action"] != "cancelled":
            wire.require(receipt["author"]["subject_id"] == self._identity._subject)
        return history


class SQLiteConsolidationJournal:
    """Fsync-backed local attempt journal. Keep its directory private and persistent.

    An exclusive POSIX file lock spans provider execution. Crash releases the lock,
    but the durable 'calling' marker remains and prevents automatic re-execution.
    Neither API keys nor context payloads are stored. Output assertions are stored.
    """

    def __init__(self, directory):
        self.directory = Path(directory)
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        if self.directory.is_symlink() or self.directory.stat().st_mode & 0o077:
            raise ValueError("The journal requires a private non-symlink directory")

    @contextmanager
    def transaction(self):
        import fcntl

        path = self.directory / "consolidation.sqlite3"
        fd = os.open(str(self.directory / "worker.lock"), os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        connection = None
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise ConsolidationStopped("journal_in_use") from None
            if path.is_symlink():
                raise ConsolidationStopped("journal_symlink_rejected")
            # Reserve with 0600 before SQLite opens it; directory is private.
            dbfd = os.open(str(path), os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            os.close(dbfd)
            connection = sqlite3.connect(str(path), isolation_level=None)
            connection.execute("PRAGMA journal_mode=DELETE")
            connection.execute("PRAGMA synchronous=FULL")
            connection.execute("CREATE TABLE IF NOT EXISTS attempts (binding TEXT NOT NULL, job TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(binding, job))")
            yield _JournalSession(connection)
        finally:
            if connection is not None:
                connection.close()
            os.close(fd)


class _JournalSession:
    def __init__(self, connection):
        self.connection = connection

    def load(self, binding, job):
        row = self.connection.execute("SELECT state FROM attempts WHERE binding=? AND job=?", (binding, job)).fetchone()
        return None if row is None else json.loads(row[0])

    def save(self, binding, job, state):
        raw = json.dumps(_copy(state, 256 * 1024), separators=(",", ":"), allow_nan=False)
        self.connection.execute("INSERT INTO attempts VALUES (?, ?, ?) ON CONFLICT(binding, job) DO UPDATE SET state=excluded.state", (binding, job, raw))


@dataclass(frozen=True)
class ConsolidationResult:
    assertions: list
    usage: dict
    evidence_ref: str
    failed: bool = False


@dataclass(frozen=True)
class ConsolidationInput:
    job_id: str
    attempt_id: str
    spec: dict
    records: list
    renew: object


class ExternalConsolidationWorker:
    """One explicit attempt per job in a durable journal; no model retry loop.

    The application chooses ready jobs and supplies a synchronous extractor.
    Callback exceptions, process death or invalid outputs leave consumption
    uncertain. Resolve using provider evidence and the lifecycle API, then use
    a new journal for an explicitly authorized retry. Never delete a journal
    merely to clear an uncertain attempt. A callback may call input.renew().
    """

    def __init__(self, client, journal, *, worker_id):
        wire.identifier(worker_id)
        self.client, self.journal, self.worker_id = client, journal, worker_id
        self.binding_id = _hash(["qilbee.consolidation.worker.v1", client.binding_id, worker_id])

    def run_once(self, job_id, extractor):
        wire.uuid(job_id)
        with self.journal.transaction() as journal:
            state = journal.load(self.binding_id, job_id)

            def save():
                journal.save(self.binding_id, job_id, state)

            if state is None:
                current = self.client.inspect(job_id)
                if current["job"]["status"] != "ready" or current["source_failure"] is not None:
                    raise ConsolidationStopped("job_not_ready")
                state = {"stage": "claiming", "command": {"type": "claim", "job_id": job_id,
                         "expected_revision": current["job"]["revision"], "worker_id": self.worker_id},
                         "key": "worker-claim-" + str(uuid4())}
                save()
            if state["stage"] == "claiming":
                receipt = self.client.command(state["command"], idempotency_key=state["key"])
                current = self.client.inspect(job_id)
                job = current["job"]
                if not current["lease_active"] or job["revision"] != receipt["revision"]:
                    raise ConsolidationStopped("claim_no_longer_current")
                attempt = job["attempts"][-1]
                wire.require(attempt["credential_id"] == receipt["author"]["credential_id"] and attempt["worker_id"] == self.worker_id)
                state = {"stage": "claimed", "revision": job["revision"], "fence": attempt["fence"], "spec": job["spec"]}
                save()
            if state["stage"] == "calling":
                raise ConsolidationStopped("provider_outcome_unknown")
            if state["stage"] == "claimed":
                context = self.client.context(job_id, state["revision"], state["fence"])
                actual = [{"record_id": r["record_id"], "revision": r["revision"]} for r in context["records"]]
                wire.require(actual == state["spec"]["sources"], "context_manifest_mismatch")
                state["stage"] = "calling"
                save()  # This intent is durable BEFORE any provider side effect.

                def renew():
                    operation = {"type": "renew", "job_id": job_id, "expected_revision": state["revision"], "fence": state["fence"]}
                    receipt = self.client.command(operation, idempotency_key="worker-renew-" + state["fence"] + "-" + str(state["revision"]))
                    state["revision"] = receipt["revision"]
                    save()
                    return receipt

                try:
                    result = extractor(ConsolidationInput(job_id, state["fence"], deepcopy(state["spec"]), deepcopy(context["records"]), renew))
                except Exception:
                    raise ConsolidationStopped("provider_outcome_unknown") from None
                if not isinstance(result, ConsolidationResult):
                    raise ConsolidationStopped("invalid_provider_result")
                wire.identifier(result.evidence_ref, 2048)
                _usage(result.usage)
                wire.require(type(result.failed) is bool and isinstance(result.assertions, list))
                wire.require(len(result.assertions) <= state["spec"]["max_relations"])
                wire.require(not result.failed or not result.assertions)
                for assertion in result.assertions:
                    wire.require(isinstance(assertion, dict))
                    wire.require(assertion.get("source") in actual and assertion.get("target") in actual)
                    wire.require(assertion["source"] != assertion["target"])
                operation = {"type": "fail" if result.failed else "publish", "job_id": job_id,
                             "expected_revision": state["revision"], "fence": state["fence"],
                             "usage": result.usage, "evidence_ref": result.evidence_ref}
                if not result.failed:
                    operation["assertions"] = result.assertions
                command = _copy(operation, 60 * 1024)
                state = {"stage": "prepared", "command": command, "key": "worker-result-" + state["fence"]}
                save()  # Publication may be retried from this exact durable output.
            if state["stage"] == "prepared":
                receipt = self.client.command(state["command"], idempotency_key=state["key"])
                state = {"stage": "complete", "receipt": receipt}
                save()
            wire.require(state["stage"] == "complete")
            # A stored receipt never substitutes for current authorization/state.
            return {"receipt": deepcopy(state["receipt"]), "inspection": self.client.inspect(job_id)}
