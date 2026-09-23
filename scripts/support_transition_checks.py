#!/usr/bin/env python3
"""Checks for observed-cohort labeling and auditable support losses/recoveries."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from evaluate_graph_retrieval import evaluation_stage
from graph_report_evidence import support_transitions, verify_support_transitions
from graph_retrieval_checks import fixture
from path_strength_contract import verify_path_strength_protocol, REGRESSION_PROTOCOL


def observations():
    queries = {qid: {"judgments": labels} for qid, labels in {
        "loss": {"a": 1, "b": 2, "noise": 0}, "gain": {"a": 1},
        "partial": {"a": 1, "b": 1}, "empty_labels": {"noise": 0},
    }.items()}
    ranked = {"loss": (["a", "b"], ["noise"]), "gain": (["noise"], ["a"]),
              "partial": (["a", "b"], ["a"]), "empty_labels": ([], ["noise"])}
    rows = [{"query_id": qid, "method": method, "ranked": pair[i]}
            for qid, pair in ranked.items() for i, method in enumerate(("base", "candidate"))]
    return rows, {"support_comparisons": [{"candidate": "candidate", "baseline": "base"}]}, queries


class SupportChecks(unittest.TestCase):
    def test_losses_gains_partial_support_and_unjudged_queries_are_distinct(self):
        rows, protocol, queries = observations()
        value = support_transitions(rows, protocol, queries)[0]
        self.assertEqual(value["query_ids"]["lost_all_labeled_support"], ["loss"])
        self.assertEqual(value["query_ids"]["lost_complete_support"], ["loss", "partial"])
        self.assertEqual(value["query_ids"]["gained_first_labeled_support"], ["gain"])
        self.assertEqual(value["query_ids"]["no_positive_judgments"], ["empty_labels"])
        self.assertEqual(value["query_ids"]["candidate_no_labeled_support"], ["loss"])
        self.assertEqual(value["changed_supports"][-1], {"query_id": "partial", "lost": ["b"], "gained": [], "retained": ["a"]})
        self.assertEqual(support_transitions(list(reversed(rows)), protocol, queries)[0], value)

    def test_duplicate_and_missing_rows_are_rejected(self):
        rows, protocol, queries = observations()
        for changed in (rows + [rows[0]], rows[:-1]):
            with self.assertRaises(ValueError):
                support_transitions(changed, protocol, queries)

    def test_forged_transition_summary_cannot_hide_a_loss(self):
        rows, protocol, queries = observations()
        report = {"support_transitions": support_transitions(rows, protocol, queries)}
        verify_support_transitions(report, rows, protocol, queries)
        report["support_transitions"][0]["query_ids"]["lost_all_labeled_support"] = []
        report["support_transitions"][0]["counts"]["lost_all_labeled_support"] = 0
        with self.assertRaises(ValueError):
            verify_support_transitions(report, rows, protocol, queries)

    def test_regression_identity_does_not_claim_reserved_confirmation(self):
        protocol = json.loads(REGRESSION_PROTOCOL.read_text())
        self.assertEqual(evaluation_stage(protocol), "observed_regression")
        for split in ("development", "reserved", "regression"):
            with self.assertRaises(ValueError):
                evaluation_stage(dict(protocol, split=split))

    def test_exact_history_cohort_and_comparison_contract_is_required(self):
        value, graph = fixture()
        protocol = json.loads(REGRESSION_PROTOCOL.read_text())
        protocol.update(source_sha256=graph["source_sha256"], measured_query_ids=["test"],
                        warmup_query_ids=["development"], documents=2)
        value["provenance"] = {"selection_manifest_sha256": protocol["selection_manifest_sha256"],
                               "split_mapping": protocol["split_mapping"]}
        # Changing provenance requires an explicit source identity, not inferred compatibility.
        from evaluate_retrieval import digest
        source = copy.deepcopy(value)
        source.pop("space"); source.pop("embedding_provenance")
        for group in ("documents", "queries"):
            for row in source[group]: row.pop("vector")
        protocol["source_sha256"] = digest(source)
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory) / "contract.json"; p.write_text(json.dumps(protocol))
            with patch("path_strength_contract.REGRESSION_PROTOCOL", p):
                verify_path_strength_protocol(protocol, value)
                for key, item in (("support_comparisons", []), ("repetitions", 1),
                                  ("scope_of_evidence", "Independent confirmation"),
                                  ("measured_query_ids", ["development"])):
                    with self.subTest(key=key), self.assertRaises(ValueError):
                        verify_path_strength_protocol(dict(protocol, **{key: item}), value)
                changed = copy.deepcopy(value); changed["queries"][1]["id"] = "other"
                with self.assertRaises(ValueError): verify_path_strength_protocol(protocol, changed)


if __name__ == "__main__":
    unittest.main()
