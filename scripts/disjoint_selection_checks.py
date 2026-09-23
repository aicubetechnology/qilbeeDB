#!/usr/bin/env python3
"""Adversarial selection checks; synthetic metadata, no model calls."""
import copy
import hashlib
import json
import unittest
from unittest.mock import patch

import select_disjoint_musique as selection


def row(hop, offset):
    ids = list(range(offset, offset + hop))
    return {"id": f"{hop}hop1__" + "_".join(map(str, ids)),
            "question": f"Full question {offset}", "answer": f"Final answer {offset}",
            "answer_aliases": [], "answerable": True,
            "paragraphs": [{"idx": i, "title": f"Title {n}", "paragraph_text": f"Support text {n}", "is_supporting": True} for i, n in enumerate(ids)],
            "question_decomposition": [{"id": n, "question": f"Subquestion {n}", "answer": f"Answer {n}", "paragraph_support_idx": i} for i, n in enumerate(ids)]}


class DisjointSelectionChecks(unittest.TestCase):
    def setup_rows(self):
        return [row(h, h * 100 + i * 10) for h in (2, 3, 4) for i in range(3)]

    def test_order_independence_and_no_quality_score_selection(self):
        rows = self.setup_rows()
        expected = selection.select_rows(rows, [row(2, 900)], "frozen", {h: 1 for h in "234"})
        shuffled = copy.deepcopy(list(reversed(rows)))
        for i, r in enumerate(shuffled):
            r["retrieval_score"] = i * 100
        self.assertEqual(expected, selection.select_rows(shuffled, [row(2, 900)], "frozen", {h: 1 for h in "234"}))
        picked = {qid for ids in expected["selected_ids"].values() for qid in ids}
        seen = set()
        for r in rows:
            if r["id"] in picked:
                keys = selection.overlap_keys(r)
                self.assertFalse(keys & seen)
                seen.update(keys)

    def test_prior_alias_subquestion_and_support_overlap_are_excluded(self):
        prior = row(2, 900)
        for field in ("alias", "subquestion", "support", "component", "question"):
            rows = self.setup_rows()
            victim = rows[0]
            if field == "alias": victim["answer_aliases"] = [prior["answer"].upper()]
            elif field == "subquestion": victim["question_decomposition"][0]["question"] = prior["question_decomposition"][0]["question"]
            elif field == "support":
                victim["paragraphs"][0].update(title=prior["paragraphs"][0]["title"], paragraph_text=prior["paragraphs"][0]["paragraph_text"])
            elif field == "component":
                victim["id"] = "2hop1__900_201"
                victim["question_decomposition"][0]["id"] = 900
            else: victim["question"] = prior["question"]
            result = selection.select_rows(rows, [prior], "frozen", {h: 1 for h in "234"})
            self.assertNotIn(victim["id"], result["selected_ids"]["2"])

    def test_within_cohort_overlap_cannot_satisfy_quota_twice(self):
        rows = self.setup_rows()
        for r in rows[:3]: r["answer"] = "Shared answer"
        with self.assertRaisesRegex(ValueError, "no quota was relaxed"):
            selection.select_rows(rows, [], "frozen", {"2": 2, "3": 1, "4": 1})

    def test_no_silent_quota_relaxation_and_strict_identity_checks(self):
        rows = self.setup_rows()
        with self.assertRaisesRegex(ValueError, "4-hop"):
            selection.select_rows(rows, [], "seed", {"2": 1, "3": 1, "4": 4})
        with self.assertRaises(ValueError): selection.select_rows(rows + [rows[0]], [], "seed", {h: 1 for h in "234"})
        for quotas in ({"2": True, "3": 1, "4": 1}, {"2": 0, "3": 1, "4": 1}):
            with self.assertRaises(ValueError): selection.select_rows(rows, [], "seed", quotas)
        for change in (lambda r: r["question_decomposition"][0].update(id=999),
                       lambda r: r["question_decomposition"][0].update(paragraph_support_idx=999),
                       lambda r: r["paragraphs"][0].update(is_supporting=False)):
            bad = copy.deepcopy(rows[0]); change(bad)
            with self.assertRaises(ValueError): selection.overlap_keys(bad)

    def test_punctuation_only_aliases_are_preserved_and_empty_values_rejected(self):
        value = row(2, 900)
        value["answer_aliases"] = [" + ", "＄"]
        keys = selection.overlap_keys(value)
        self.assertIn("answer:+", keys)
        self.assertIn("answer:$", keys)
        for invalid in ([" "], [None], "answer"):
            value["answer_aliases"] = invalid
            with self.assertRaises(ValueError): selection.overlap_keys(value)

    def test_full_replay_binds_sources_prior_fixtures_and_metadata(self):
        rows = self.setup_rows(); previous = row(2, 900)
        raw = {"musique_ans_v1.0_train.jsonl": json.dumps(previous).encode(),
               "musique_ans_v1.0_dev.jsonl": "\n".join(map(json.dumps, rows)).encode()}
        hashes = {name: hashlib.sha256(value).hexdigest() for name, value in raw.items()}
        prior = json.dumps({"queries": [{"id": previous["id"], "text": previous["question"]}]}).encode()
        with patch.object(selection, "SOURCE_HASHES", hashes):
            manifest = selection.build(raw, [prior], "dev", "seed", {h: 1 for h in "234"})
            self.assertEqual(manifest, selection.verify(manifest, raw, [prior]))
            for mutate in (lambda m: m.update(retrieval_results_used=True),
                           lambda m: m["selected_ids"]["2"].append(rows[0]["id"]),
                           lambda m: m.update(previous_unique_queries=0),
                           lambda m: m.update(selected_overlap_keys_sha256="wrong"),
                           lambda m: m["source_file_sha256"].update({"musique_ans_v1.0_dev.jsonl": "wrong"})):
                changed = copy.deepcopy(manifest); mutate(changed)
                with self.assertRaises(ValueError): selection.verify(changed, raw, [prior])
            with self.assertRaises(ValueError): selection.build(raw, [], "dev", "seed", {h: 1 for h in "234"})
            changed = dict(raw); changed["musique_ans_v1.0_dev.jsonl"] += b"\n"
            with self.assertRaisesRegex(ValueError, "digest"):
                selection.build(changed, [prior], "dev", "seed", {h: 1 for h in "234"})
            wrong = json.dumps({"queries": [{"id": previous["id"], "text": "Wrong question"}]}).encode()
            with self.assertRaisesRegex(ValueError, "Prior query"):
                selection.build(raw, [wrong], "dev", "seed", {h: 1 for h in "234"})


if __name__ == "__main__":
    unittest.main()
