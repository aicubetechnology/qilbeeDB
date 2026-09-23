#!/usr/bin/env python3
"""Adversarial checks for explicit reserved graph cohorts; no model calls."""
import copy
import unittest
from import_reserved_musique_graph import selected_rows, materialize
from evaluate_graph_retrieval import evaluation_stage


class ReservedSelectionChecks(unittest.TestCase):
    def fixture(self):
        prior = {"queries": [{"id": "2hop__1_2", "text": "Previously used"}]}
        rows = [{"id": qid, "question": "Question " + str(i), "answerable": True}
                for i, qid in enumerate(["2hop__10_11", "3hop1__12_13_14", "4hop1__15_16_17_18"])]
        manifest = {"selection_version": "identifier_hash_disjoint_components_v1",
                    "labels_used_for_selection": False, "quotas": {h: 1 for h in "234"},
                    "selected_ids": {h: [rows[i]["id"]] for i, h in enumerate("234")}}
        return manifest, prior, rows

    def test_exact_selection_is_order_independent_and_labels_do_not_select(self):
        m, p, rows = self.fixture()
        chosen = selected_rows(m, p, rows)
        self.assertEqual(chosen, selected_rows(m, p, list(reversed(rows))))
        changed = copy.deepcopy(rows)
        for r in changed:
            r.update(answer="unused", paragraphs=[{"is_supporting": False}])
        self.assertEqual([r["id"] for r in chosen], [r["id"] for r in selected_rows(m, p, changed)])

    def test_reserved_protocol_cannot_be_relabelled_as_development(self):
        self.assertEqual(evaluation_stage({"protocol_version": "graph_base_preserving_reserved_v1", "split": "test"}), "reserved_comparison")
        with self.assertRaises(ValueError):
            evaluation_stage({"protocol_version": "graph_base_preserving_reserved_v1", "split": "development"})

    def test_source_hashes_are_checked_before_materialization(self):
        import hashlib
        with self.assertRaisesRegex(ValueError, "fixture digest"):
            materialize({"excluded_fixture_sha256": "wrong"}, b"{}", b"", b"")
        with self.assertRaisesRegex(ValueError, "Official source digest"):
            materialize({"excluded_fixture_sha256": hashlib.sha256(b"{}").hexdigest()}, b"{}", b"", b"")

    def test_rejects_previous_components_and_ids(self):
        m, p, rows = self.fixture()
        for qid in ["2hop__1_2", "2hop__1_99"]:
            altered = copy.deepcopy(rows); altered[0]["id"] = qid
            changed = copy.deepcopy(m); changed["selected_ids"]["2"] = [qid]
            with self.subTest(qid=qid), self.assertRaises(ValueError): selected_rows(changed, p, altered)

    def test_rejects_duplicate_questions_in_both_directions(self):
        m, p, rows = self.fixture()
        for text in ["  PREVIOUSLY USED ", rows[1]["question"].upper()]:
            altered = copy.deepcopy(rows); altered[0]["question"] = text
            with self.assertRaises(ValueError): selected_rows(m, p, altered)

    def test_rejects_manifest_drift_and_duplicate_source_ids(self):
        m, p, rows = self.fixture()
        mutations = [lambda x: x.update(labels_used_for_selection=True),
                     lambda x: x["quotas"].update({"2": True}),
                     lambda x: x["quotas"].update({"2": 2}),
                     lambda x: x["selected_ids"].update({"2": ["2hop__99_100"]}),
                     lambda x: x["selected_ids"].update({"2": [rows[1]["id"]]})]
        for mutate in mutations:
            changed = copy.deepcopy(m); mutate(changed)
            with self.assertRaises(ValueError): selected_rows(changed, p, rows)
        with self.assertRaises(ValueError): selected_rows(m, p, rows + [rows[0]])


if __name__ == "__main__":
    unittest.main()
