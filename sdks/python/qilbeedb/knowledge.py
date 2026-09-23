"""Opt-in knowledge selection v3 using the standard-library HTTP transport.

This client checks protocol consistency, not the truth of knowledge or the
server's ledger digests. It never executes instructions or chooses a fallback.
"""
from copy import deepcopy
from . import _consumer_wire as wire
from .consumer import VerifiedMemoryConsumer

SELECTION_VERSION = "active_knowledge_bound_v1"
_LIMITS = {
    "candidate_records": 1000, "index_entries": 4096,
    "learning_bytes": 16777216, "dependency_records": 4096,
    "dependency_bytes": 16777216,
}
_STOPS = {
    "index_entry_limit", "candidate_record_limit", "learning_byte_limit",
    "dependency_record_limit", "dependency_byte_limit", "dependency_depth_limit",
    "dependency_node_limit",
}


def _identities(values):
    wire.require(isinstance(values, list) and len(values) <= 32, "invalid_tool_identities")
    result = deepcopy(values)
    names = set()
    for value in result:
        wire.object_fields(value, "name schema_revision implementation_revision environment_revision")
        wire.identifier(value["name"], 512)
        wire.identifier(value["schema_revision"], 512)
        wire.require(value["name"] not in names, "duplicate_tool_name")
        names.add(value["name"])
        implementation, environment = value["implementation_revision"], value["environment_revision"]
        wire.require((implementation is None) == (environment is None), "incomplete_tool_identity")
        if implementation is not None:
            wire.identifier(implementation, 512)
            wire.identifier(environment, 512)
    return sorted(result, key=lambda value: value["name"].encode("utf-8"))


class KnowledgeSelectionClient:
    """Read evidence-bound knowledge without automatic retries or v2 downgrade.

    Credentials may be supplied as a string or a callable. The expected company
    and subject are checked on each operation; the server authorizes the scope.
    Results are observations, not durable reservations or execution permissions.
    """
    def __init__(self, base_url, api_key, *, tenant_id, subject_id, scope,
                 timeout=10.0, max_response_bytes=16 * 1024 * 1024):
        self._identity = VerifiedMemoryConsumer(
            base_url, api_key, tenant_id=tenant_id, subject_id=subject_id,
            scope=scope, consumer_id="knowledge-selection", timeout=timeout,
            max_response_bytes=max_response_bytes,
        )

    def select(self, *, policy_id, context_id, external_tool_identities,
               max_instruction_bytes, work_limits, selection_version):
        """Return the complete validated response, including coverage and work.

        Inspect result.type (procedure, baseline, incomplete) in application code.
        Passing another method or omitting a limit is an error, not a fallback.
        """
        wire.require(selection_version == SELECTION_VERSION, "unsupported_selection_version")
        wire.identifier(policy_id, 512)
        wire.identifier(context_id, 512)
        wire.integer(max_instruction_bytes, 0, 65536)
        wire.require(isinstance(work_limits, dict), "invalid_work_limits")
        limits = dict(work_limits)
        wire.object_fields(limits, " ".join(_LIMITS))
        for name, maximum in _LIMITS.items():
            wire.integer(limits[name], 1, maximum)
        identities = _identities(external_tool_identities)
        body = {
            "contract_version": 3, "selection_version": selection_version,
            "scope": deepcopy(self._identity._scope), "policy_id": policy_id,
            "context_id": context_id, "max_instruction_bytes": max_instruction_bytes,
            "external_tool_identities": identities, "work_limits": limits,
        }
        with self._identity._operation() as token:
            response = self._identity._http.request(token, "/api/v1/learning/knowledge/select", body)
        self._validate(response, body)
        return response

    def inspect(self, procedure_id):
        """Read current qualification and source eligibility, retaining audit data.

        A false reuse flag is a valid result. A transport or authorization error
        supplies no current observation; this method never returns cached data.
        """
        wire.identifier(procedure_id, 512)
        body = {"contract_version": 2, "scope": deepcopy(self._identity._scope),
                "procedure_id": procedure_id}
        with self._identity._operation() as token:
            response = self._identity._http.request(
                token, "/api/v1/learning/knowledge/inspect", body)
        wire.object_fields(response, "contract_version inspection")
        wire.require(type(response["contract_version"]) is int and response["contract_version"] == 2)
        self._inspection(response["inspection"], procedure_id)
        return response

    def _inspection(self, knowledge, procedure_id):
        wire.object_fields(knowledge, "receipt procedure qualification_active evidence eligible_for_knowledge_reuse")
        for field in ("qualification_active", "eligible_for_knowledge_reuse"):
            wire.require(type(knowledge[field]) is bool)
        receipt, procedure = knowledge["receipt"], knowledge["procedure"]
        wire.require(isinstance(receipt, dict) and isinstance(procedure, dict))
        wire.require(type(receipt.get("schema_version")) is int and receipt.get("schema_version") == 2)
        wire.require(receipt.get("tenant") == self._identity._tenant, "knowledge_tenant_mismatch")
        wire.digest(receipt.get("receipt_digest"))
        proposal, current = receipt.get("request"), procedure.get("record")
        wire.require(isinstance(proposal, dict) and isinstance(current, dict))
        wire.require(proposal.get("id") == procedure_id, "knowledge_procedure_mismatch")
        current_proposal, scope = current.get("proposal"), current.get("scope")
        wire.require(isinstance(current_proposal, dict) and isinstance(scope, dict))
        wire.require(scope.get("tenant") == self._identity._tenant, "knowledge_tenant_mismatch")
        for field in ("id", "instructions"):
            wire.require(isinstance(proposal.get(field), str))
            wire.require(current_proposal.get(field) == proposal[field], "knowledge_binding_mismatch")
        sources = proposal.get("memory_sources")
        wire.require(isinstance(sources, list) and 1 <= len(sources) <= 16)
        seen = set()
        for source in sources:
            wire.object_fields(source, "record_id revision")
            wire.uuid(source["record_id"])
            wire.integer(source["revision"], 1)
            wire.require(source["record_id"] not in seen)
            seen.add(source["record_id"])
        state = current.get("state")
        wire.require(isinstance(state, str) and state in ("Candidate", "Active", "Rejected", "Suspended"))
        wire.require(knowledge["qualification_active"] == (state == "Active"))
        evidence = knowledge["evidence"]
        wire.object_fields(evidence, "eligible evaluated_at_millis first_failure all_dependencies_checked dependency_work")
        for field in ("eligible", "all_dependencies_checked"):
            wire.require(type(evidence[field]) is bool)
        wire.integer(evidence["evaluated_at_millis"], 0, 2**63 - 1)
        wire.object_fields(evidence["dependency_work"], "records_examined bytes_examined")
        for value in evidence["dependency_work"].values():
            wire.integer(value)
        failure = evidence["first_failure"]
        if failure is None:
            wire.require(evidence["eligible"] and evidence["all_dependencies_checked"])
        else:
            wire.object_fields(failure, "record_id expected_revision actual_revision reason")
            wire.uuid(failure["record_id"])
            for field in ("expected_revision", "actual_revision"):
                if failure[field] is not None:
                    wire.integer(failure[field], 1)
            wire.require(isinstance(failure["reason"], str) and failure["reason"] in (
                "deleted", "expired", "rejected", "source_missing", "source_revision_changed",
                "dependency_cycle", "depth_limit", "node_limit"))
            wire.require(not evidence["eligible"] and not evidence["all_dependencies_checked"])
        wire.require(knowledge["eligible_for_knowledge_reuse"] == (
            knowledge["qualification_active"] and evidence["eligible"] and evidence["all_dependencies_checked"]))

    def _validate(self, response, request):
        wire.object_fields(response, "contract_version selection_version scope policy_id context_id evaluated_at_millis index_generation result coverage work")
        wire.require(type(response["contract_version"]) is int and response["contract_version"] == 3)
        for field in ("selection_version", "policy_id", "context_id"):
            wire.require(response[field] == request[field], "selection_binding_mismatch")
        wire.require(wire.scope(response["scope"]) == request["scope"], "selection_scope_mismatch")
        wire.integer(response["evaluated_at_millis"], 0, 2**63 - 1)
        wire.uuid(response["index_generation"])
        coverage, work, result = response["coverage"], response["work"], response["result"]
        wire.object_fields(coverage, "decision_complete index_exhausted stop_reason")
        wire.require(type(coverage["decision_complete"]) is bool and type(coverage["index_exhausted"]) is bool)
        wire.object_fields(work, "index_entries_examined candidate_records_examined learning_bytes_inspected learning_lookahead_bytes dependency_records_examined dependency_bytes_inspected dependency_lookahead_bytes eligible_candidates")
        for value in work.values():
            wire.integer(value)
        for count, limit in (
            ("index_entries_examined", "index_entries"),
            ("candidate_records_examined", "candidate_records"),
            ("learning_bytes_inspected", "learning_bytes"),
            ("dependency_records_examined", "dependency_records"),
            ("dependency_bytes_inspected", "dependency_bytes"),
        ):
            wire.require(work[count] <= request["work_limits"][limit], "selection_work_exceeded")
        wire.require(work["eligible_candidates"] <= 1)
        wire.require(work["eligible_candidates"] <= work["candidate_records_examined"] <= work["index_entries_examined"])
        wire.require(isinstance(result, dict))
        kind = result.get("type")
        if kind == "procedure":
            wire.object_fields(result, "type knowledge")
            wire.require(coverage["decision_complete"] and coverage["stop_reason"] == "first_eligible")
            wire.require(work["eligible_candidates"] == 1)
            self._knowledge(result["knowledge"], request, response["evaluated_at_millis"])
        elif kind == "baseline":
            wire.object_fields(result, "type baseline_revision reason")
            wire.identifier(result["baseline_revision"], 512)
            wire.require(result["reason"] == "no_eligible_bound_procedure")
            wire.require(coverage["decision_complete"] and coverage["index_exhausted"] and coverage["stop_reason"] == "index_exhausted")
            wire.require(work["eligible_candidates"] == 0)
        elif kind == "incomplete":
            wire.object_fields(result, "type baseline_revision reason")
            wire.identifier(result["baseline_revision"], 512)
            wire.require(isinstance(result["reason"], str) and result["reason"] in _STOPS)
            wire.require(not coverage["decision_complete"] and coverage["stop_reason"] == result["reason"])
            wire.require(work["eligible_candidates"] == 0)
        else:
            raise wire.ConsumerProtocolError("unknown_selection_outcome")

    def _knowledge(self, knowledge, request, observed_at):
        wire.object_fields(knowledge, "receipt procedure qualification_active evidence eligible_for_knowledge_reuse")
        wire.require(knowledge["qualification_active"] is True and knowledge["eligible_for_knowledge_reuse"] is True)
        evidence = knowledge["evidence"]
        wire.require(isinstance(evidence, dict))
        wire.require(evidence.get("eligible") is True and evidence.get("all_dependencies_checked") is True)
        wire.integer(evidence.get("evaluated_at_millis"), 0, 2**63 - 1)
        wire.require(evidence.get("first_failure") is None and evidence.get("evaluated_at_millis") == observed_at)
        receipt, procedure = knowledge["receipt"], knowledge["procedure"]
        wire.require(isinstance(receipt, dict) and isinstance(procedure, dict))
        wire.require(type(receipt.get("schema_version")) is int and receipt.get("schema_version") == 2 and receipt.get("tenant") == self._identity._tenant)
        wire.digest(receipt.get("receipt_digest"))
        proposal, current = receipt.get("request"), procedure.get("record")
        wire.require(isinstance(proposal, dict) and isinstance(current, dict))
        wire.require(current.get("state") == "Active")
        for field in ("policy_id", "context_id"):
            wire.require(proposal.get(field) == request[field], "knowledge_binding_mismatch")
        wire.identifier(proposal.get("id"), 512)
        sources = proposal.get("memory_sources")
        wire.require(isinstance(sources, list) and 1 <= len(sources) <= 16)
        source_ids = set()
        for source in sources:
            wire.object_fields(source, "record_id revision")
            wire.uuid(source["record_id"])
            wire.integer(source["revision"], 1)
            wire.require(source["record_id"] not in source_ids)
            source_ids.add(source["record_id"])
        instructions = proposal.get("instructions")
        wire.require(isinstance(instructions, str) and len(instructions.encode("utf-8")) <= request["max_instruction_bytes"])
        tools = proposal.get("external_tools")
        wire.require(isinstance(tools, list))
        identities = []
        for tool in tools:
            wire.require(isinstance(tool, dict))
            identities.append({key: tool.get(key) for key in (
                "name", "schema_revision", "implementation_revision", "environment_revision")})
        wire.require(_identities(identities) == request["external_tool_identities"], "knowledge_tool_mismatch")
        current_proposal, scope = current.get("proposal"), current.get("scope")
        wire.require(isinstance(current_proposal, dict) and isinstance(scope, dict))
        wire.require(scope.get("tenant") == self._identity._tenant)
        wire.require(current_proposal.get("id") == proposal.get("id") and current_proposal.get("instructions") == instructions)
