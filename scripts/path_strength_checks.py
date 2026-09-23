#!/usr/bin/env python3
"""Adversarial validation of strength evidence and the frozen experiment identity."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from evaluate_retrieval import digest
from graph_retrieval_checks import proof_fixture
from graph_evaluation_contract import graph_profile, validate_page
from path_strength_contract import verify_path_strength_protocol, PROTOCOL
from evaluate_graph_retrieval import request_for, evaluation_stage
from export_graph_evaluation import export


def strength_fixture():
    fixture, graph, state, protocol, result = proof_fixture()
    method = "graph_hybrid_strength"
    protocol["graph_profiles"][method] = "typed_path_strength_v1"
    protocol["graph_seed_hybrid_version"] = "weighted_rrf_v1"
    page = result["page"]
    page["ranking"] = graph_profile("typed_path_strength_v1")
    from evaluate_retrieval import PROFILES
    page["seed"].update(mode="hybrid", hybrid_profile=PROFILES["weighted_rrf_v1"], embedding_space=fixture["space"])
    hit = page["hits"][0]
    hit["graph"]["contribution"] = hit["score"] = 1 / 12
    return fixture, state, protocol, result, method


class StrengthEvidence(unittest.TestCase):
    def test_actual_path_magnitude_is_used_not_reciprocal_graph_rank(self):
        fixture, state, protocol, result, method = strength_fixture()
        validate_page(result, fixture, state, fixture["queries"][0], method, protocol)
        changed = copy.deepcopy(result)
        changed["page"]["hits"][0]["graph"]["rank"] = 8
        # Ranking metadata can differ while the path's contribution stays fixed.
        validate_page(changed, fixture, state, fixture["queries"][0], method, protocol)
        changed["page"]["hits"][0]["graph"]["contribution"] = 0.5 / 10
        changed["page"]["hits"][0]["score"] = 0.5 / 10
        with self.assertRaises(ValueError):
            validate_page(changed, fixture, state, fixture["queries"][0], method, protocol)

    def test_coherent_forged_strength_cannot_bypass_path_reconstruction(self):
        fixture, state, protocol, result, method = strength_fixture()
        for strength in (0, 0.3, -1, float("nan"), float("inf")):
            changed = copy.deepcopy(result)
            hit = changed["page"]["hits"][0]
            hit["graph"]["strength"] = strength
            hit["graph"]["contribution"] = hit["score"] = 0.5 * strength
            with self.subTest(strength=strength), self.assertRaises(ValueError):
                validate_page(changed, fixture, state, fixture["queries"][0], method, protocol)

    def test_missing_channel_does_not_redistribute_weight(self):
        fixture, state, protocol, result, method = strength_fixture()
        result["page"]["hits"][0]["score"] = 1 / 6
        result["page"]["hits"][0]["graph"]["contribution"] = 1 / 6
        with self.assertRaises(ValueError):
            validate_page(result, fixture, state, fixture["queries"][0], method, protocol)

    def test_existing_rank_fusion_still_rejects_strength_substitution(self):
        fixture, graph, state, protocol, result = proof_fixture()
        method = "graph_lexical_balanced"
        validate_page(result, fixture, state, fixture["queries"][0], method, protocol)
        hit = result["page"]["hits"][0]
        hit["score"] = hit["graph"]["contribution"] = 0.75 * hit["graph"]["strength"]
        with self.assertRaises(ValueError):
            validate_page(result, fixture, state, fixture["queries"][0], method, protocol)

    def test_versioned_protocol_pins_weights_budgets_cohort_and_stage(self):
        fixture, graph, state, unused, result = proof_fixture()
        protocol = json.loads(PROTOCOL.read_text())
        self.assertEqual(evaluation_stage(protocol), "development")
        route, request = request_for("graph_hybrid_strength", fixture["queries"][0], fixture, state, protocol)
        self.assertEqual(route, "/api/v1/memory/search/graph")
        self.assertEqual(request["query"]["ranking_version"], "typed_path_strength_v1")
        self.assertEqual(request["query"]["seed"]["ranking_version"], "weighted_rrf_v1")
        protocol["source_sha256"] = graph["source_sha256"]
        protocol["measured_query_ids"] = ["development"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "protocol.json"
            path.write_text(json.dumps(protocol))
            with patch("path_strength_contract.PROTOCOL", path):
                verify_path_strength_protocol(protocol, fixture)
                for key, value in (("split", "test"), ("repetitions", 2), ("client_concurrency", True), ("scan_limit", 9999), ("graph_seed_hybrid_version", "weighted_rrf_v2"), ("measured_query_ids", ["test"]), ("unexpected", 1)):
                    changed = dict(protocol, **{key: value})
                    with self.subTest(key=key), self.assertRaises(ValueError):
                        verify_path_strength_protocol(changed, fixture)
                changed = copy.deepcopy(fixture)
                changed["documents"][0]["text"] += " altered"
                with self.assertRaises(ValueError):
                    verify_path_strength_protocol(protocol, changed)
                # Updating an attacker's plan hash does not change the checked-in contract.
                changed = dict(protocol, repetitions=2)
                report = {"status": "completed", "failures": [], "verified_fences_unchanged": True,
                          "verified_current_sources_and_relations": True,
                          "plan": {"fixture_sha256": digest(fixture), "protocol": changed, "protocol_sha256": digest(changed)}}
                with self.assertRaisesRegex(ValueError, "frozen protocol"):
                    export(report, fixture)


if __name__ == "__main__":
    unittest.main()
