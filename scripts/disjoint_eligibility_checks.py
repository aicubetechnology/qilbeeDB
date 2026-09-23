#!/usr/bin/env python3
"""Verify pre-selection capacity, exclusions and replay without retrieval."""
import copy
import hashlib
import json
import unittest
import tempfile
import contextlib
import io
from pathlib import Path
from unittest.mock import patch

from disjoint_selection_checks import row
import select_disjoint_musique as selection


class EligibilityChecks(unittest.TestCase):
    def fixture(self):
        previous = row(2, 900)
        rows = [row(h, h * 100 + i * 10) for h in (2, 3, 4) for i in range(3)]
        raw = {"musique_ans_v1.0_train.jsonl": json.dumps(previous).encode(),
               "musique_ans_v1.0_dev.jsonl": "\n".join(map(json.dumps, rows)).encode()}
        hashes = {name: hashlib.sha256(value).hexdigest() for name, value in raw.items()}
        prior = json.dumps({"queries": [{"id": previous["id"], "text": previous["question"]}]}).encode()
        return raw, hashes, prior

    def test_eligibility_is_only_an_upper_bound(self):
        rows = [row(h, h * 100 + i * 10) for h in (2, 3, 4) for i in range(3)]
        for candidate in rows:
            candidate["answer"] = "Same answer across all candidates"
        eligible, *_ = selection.eligibility(rows, [])
        self.assertEqual(len(eligible), 9)
        with self.assertRaisesRegex(ValueError, "no quota was relaxed"):
            selection.select_rows(rows, [], "frozen", {h: 1 for h in "234"})

    def test_exclusion_categories_are_nonexclusive_and_prior_ids_take_precedence(self):
        previous = row(2, 900)
        candidate = row(3, 300)
        candidate["answer"] = previous["answer"]
        candidate["question"] = previous["question"]
        ineligible = row(4, 400)
        ineligible["answerable"] = False
        eligible, reasons, ids, _, by_hop, answerable = selection.eligibility(
            [previous, candidate, ineligible], [previous, copy.deepcopy(previous)])
        self.assertEqual(eligible, [])
        self.assertEqual(ids, {previous["id"]})
        self.assertEqual(dict(reasons), {"previous_id": 1, "answer": 1, "query": 1, "not_answerable": 1})
        self.assertEqual(dict(by_hop["3"]), {"answer": 1, "query": 1})
        self.assertEqual(dict(answerable), {"2": 1, "3": 1})

    def test_replay_rejects_changed_counts_metadata_and_added_fields(self):
        raw, hashes, prior = self.fixture()
        with patch.object(selection, "SOURCE_HASHES", hashes):
            result = selection.audit(raw, [prior], "dev")
            self.assertEqual(result["eligible_by_hop"], {h: 3 for h in "234"})
            self.assertEqual(result, selection.verify_audit(result, raw, [prior]))
            for mutate in (lambda m: m["eligible_by_hop"].update({"4": 0}),
                           lambda m: m.update(selected_queries=1),
                           lambda m: m.update(retrieval_performed=True),
                           lambda m: m.update(extra="unverified"),
                           lambda m: m.update(prior_fixture_sha256=[])):
                changed = copy.deepcopy(result)
                mutate(changed)
                with self.assertRaisesRegex(ValueError, "does not replay"):
                    selection.verify_audit(changed, raw, [prior])

    def test_sources_and_history_are_required_and_verified(self):
        raw, hashes, prior = self.fixture()
        with patch.object(selection, "SOURCE_HASHES", hashes):
            with self.assertRaisesRegex(ValueError, "previously used"):
                selection.audit(raw, [], "dev")
            changed = dict(raw)
            changed["musique_ans_v1.0_dev.jsonl"] += b"\n"
            with self.assertRaisesRegex(ValueError, "digest"):
                selection.audit(changed, [prior], "dev")
            wrong = json.loads(prior)
            wrong["queries"][0]["text"] = "Different question"
            with self.assertRaisesRegex(ValueError, "Prior query"):
                selection.audit(raw, [json.dumps(wrong).encode()], "dev")

    def test_cli_exclusive_output_replay_and_invalid_arguments(self):
        raw, hashes, prior = self.fixture()
        with tempfile.TemporaryDirectory() as directory, patch.object(selection, "SOURCE_HASHES", hashes):
            root = Path(directory)
            for name, data in raw.items():
                (root / name).write_bytes(data)
            (root / "prior.json").write_bytes(prior)
            output = root / "audit.json"
            common = ["--source-dir", str(root), "--prior-fixture", str(root / "prior.json"),
                      "--manifest", str(output)]
            def invoke(action, extra=()):
                with patch("sys.argv", ["selector", action, *common, *extra]), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    selection.main()
            invoke("audit", ["--source-split", "dev"])
            original = output.read_bytes()
            invoke("verify-audit")
            with self.assertRaises(FileExistsError):
                invoke("audit", ["--source-split", "dev"])
            self.assertEqual(original, output.read_bytes())
            for action, args in (("audit", []), ("audit", ["--source-split", "dev", "--seed", "change"]),
                                 ("audit", ["--source-split", "dev", "--seed", ""]),
                                 ("audit", ["--source-split", "dev", "--quotas", "1", "1", "1"]),
                                 ("verify-audit", ["--source-split", "dev"])):
                with self.assertRaises(SystemExit) as error:
                    invoke(action, args)
                self.assertEqual(error.exception.code, 2)
            changed = json.loads(original)
            changed["eligible_by_hop"]["4"] = 99
            output.write_text(json.dumps(changed))
            with self.assertRaisesRegex(ValueError, "does not replay"):
                invoke("verify-audit")

    def test_history_order_and_repetition_do_not_change_capacity(self):
        raw, hashes, prior = self.fixture()
        with patch.object(selection, "SOURCE_HASHES", hashes):
            result = selection.audit(raw, [prior], "dev")
            self.assertEqual(result, selection.audit(raw, [prior, prior], "dev"))
            self.assertEqual(result["selected_queries"], 0)
            self.assertFalse(result["retrieval_performed"])


if __name__ == "__main__":
    unittest.main()
