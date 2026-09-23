#!/usr/bin/env python3
"""Adversarial checks for the frozen support-disjoint reserved comparison."""
import copy
import json
import unittest

from evaluate_graph_retrieval import evaluation_stage
from evaluate_retrieval import digest
from export_graph_evaluation import export
from support_reserved_contract import PROTOCOL, verify_support_reserved


class SupportReservedChecks(unittest.TestCase):
    def fixture(self):
        protocol = json.loads(PROTOCOL.read_text())
        fixture = {"queries": [{"id": qid, "split": split} for key, split in (
            ("measured_query_ids", "test"), ("warmup_query_ids", "development")) for qid in protocol[key]],
            "documents": [None] * protocol["documents"], "provenance": {
                "selection_manifest_sha256": protocol["selection_manifest_sha256"],
                "split_mapping": protocol["split_mapping"]}}
        return protocol, fixture, {"relations": [None] * protocol["relations"]}

    def test_complete_frozen_contract_and_stage(self):
        p, f, g = self.fixture()
        verify_support_reserved(p, f, g)
        self.assertEqual(evaluation_stage(p), "reserved_comparison")
        self.assertEqual(len(p["measured_query_ids"]), 140)
        self.assertEqual(len(p["warmup_query_ids"]), 30)
        self.assertEqual(len(p["methods"]) * p["repetitions"] * 140, 2520)

    def test_rejects_candidate_budget_baseline_and_metadata_drift(self):
        p, f, g = self.fixture()
        mutations = [lambda x: x.update(split="development"),
            lambda x: x.update(graph_seed_hybrid_version="weighted_rrf_v2"),
            lambda x: x["graph_profiles"].update(graph_hybrid_balanced="typed_path_entity_v1"),
            lambda x: x["primary_comparison"].update(baseline="weighted_rrf_v2"),
            lambda x: x["methods"].remove("graph_hybrid_depth_zero"),
            lambda x: x["expansion"].update(edge_limit=512),
            lambda x: x.update(scan_bytes_limit=1, repetitions=1),
            lambda x: x.update(default_admission=True),
            lambda x: x.update(scope_of_evidence="Independent hidden test"),
            lambda x: x["measured_query_ids"].reverse(),
            lambda x: x.update(unrecognized_override=True)]
        for mutate in mutations:
            altered = copy.deepcopy(p); mutate(altered)
            with self.assertRaises(ValueError): verify_support_reserved(altered, f, g)

    def test_export_rejects_rehashed_protocol_tampering(self):
        p, f, _ = self.fixture()
        p["expansion"]["node_limit"] = 999
        report = {"status": "completed", "failures": [], "verified_fences_unchanged": True,
            "verified_current_sources_and_relations": True,
            "plan": {"fixture_sha256": digest(f), "protocol": p, "protocol_sha256": digest(p)}}
        with self.assertRaisesRegex(ValueError, "frozen versioned protocol"):
            export(report, f)

    def test_rejects_query_replacement_missing_warmup_and_wrong_provenance(self):
        p, f, g = self.fixture()
        mutations = [lambda x: x["queries"][0].update(id="replacement"),
            lambda x: x["queries"].pop(),
            lambda x: x["queries"].append(x["queries"][0]),
            lambda x: x["queries"][0].update(split="development"),
            lambda x: x["documents"].pop(),
            lambda x: x["provenance"].update(selection_manifest_sha256="wrong"),
            lambda x: x["provenance"].update(split_mapping={"test": "hidden"})]
        for mutate in mutations:
            altered = copy.deepcopy(f); mutate(altered)
            with self.assertRaises(ValueError): verify_support_reserved(p, altered, g)
        g["relations"].pop()
        with self.assertRaises(ValueError): verify_support_reserved(p, f, g)


if __name__ == "__main__":
    unittest.main()
