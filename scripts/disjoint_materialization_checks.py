#!/usr/bin/env python3
"""Adversarial corpus provenance and interrupted-bundle checks; no model calls."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from disjoint_selection_checks import row
from evaluate_retrieval import digest
import import_musique_graph
import select_disjoint_musique as selection
from materialize_disjoint_musique import materialize, receipt, write_bundle, verify_bundle


class MaterializationChecks(unittest.TestCase):
    def fixture(self, split="train", duplicate_warmup=False):
        previous = row(2, 900)
        candidates = [row(h, h * 100) for h in (2, 3, 4)]
        train = [previous] + (candidates if split == "train" else [])
        dev = candidates if split == "dev" else [row(2, 800)]
        sources = {f"musique_ans_v1.0_{name}.jsonl": "\n".join(map(json.dumps, rows)).encode()
                   for name, rows in (("train", train), ("dev", dev))}
        query = {"id": previous["id"], "text": previous["question"], "split": "development"}
        warmup = json.dumps({"queries": [query] * (2 if duplicate_warmup else 1)}).encode()
        hashes = {name: hashlib.sha256(raw).hexdigest() for name, raw in sources.items()}
        return sources, warmup, hashes

    def materialized(self, split="train"):
        sources, warmup, hashes = self.fixture(split)
        with patch.object(selection, "SOURCE_HASHES", hashes):
            manifest = selection.build(sources, [warmup], split, "fixed", {h: 1 for h in "234"})
            source, graph = materialize(manifest, sources, [warmup], warmup)
        return source, graph

    def test_exact_ids_splits_provenance_and_document_only_graph_inputs(self):
        for split in ("train", "dev"):
            sources, warmup, hashes = self.fixture(split)
            with patch.object(selection, "SOURCE_HASHES", hashes):
                manifest = selection.build(sources, [warmup], split, "fixed", {h: 1 for h in "234"})
                with patch.object(import_musique_graph, "document_graph", wraps=import_musique_graph.document_graph) as graph_builder:
                    source, graph = materialize(manifest, sources, [warmup], warmup)
                projection = graph_builder.call_args.args[0]
                self.assertTrue(all(set(d) == {"id", "title", "paragraph_text"} for d in projection))
                self.assertEqual(graph["projection"], projection)
                self.assertEqual(graph["source_sha256"], digest(source))
                self.assertEqual(source["provenance"]["selection_manifest_sha256"], digest(manifest))
                self.assertIn(f"public {split} subset", source["provenance"]["split_mapping"]["test"])
                self.assertIn("not official hidden test", source["provenance"]["split_mapping"]["test"])
                self.assertEqual({q["id"] for q in source["queries"] if q["split"] == "test"},
                                 {qid for ids in manifest["selected_ids"].values() for qid in ids})
                self.assertEqual(receipt(source, graph)["queries_by_split"], {"development": 1, "test": 3})
                self.assertTrue(all("vector" not in item for key in ("documents", "queries") for item in source[key]))

    def test_rejects_selection_tampering_and_unbound_warmup(self):
        sources, warmup, hashes = self.fixture()
        with patch.object(selection, "SOURCE_HASHES", hashes):
            manifest = selection.build(sources, [warmup], "train", "fixed", {h: 1 for h in "234"})
            changed = copy.deepcopy(manifest); changed["selected_ids"]["2"] = ["2hop1__800_801"]
            with self.assertRaises(ValueError): materialize(changed, sources, [warmup], warmup)
            with self.assertRaisesRegex(ValueError, "Warm-up fixture"):
                materialize(manifest, sources, [warmup], warmup + b"\n")
        sources, warmup, hashes = self.fixture(duplicate_warmup=True)
        with patch.object(selection, "SOURCE_HASHES", hashes):
            manifest = selection.build(sources, [warmup], "train", "fixed", {h: 1 for h in "234"})
            with self.assertRaisesRegex(ValueError, "unique"):
                materialize(manifest, sources, [warmup], warmup)

    def test_bundle_roundtrip_tampering_and_overwrite_protection(self):
        source, graph = self.materialized()
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp) / "bundle"
            write_bundle(directory, source, graph)
            self.assertEqual(verify_bundle(directory, source, graph), receipt(source, graph))
            before = {p.name: p.read_bytes() for p in directory.iterdir()}
            with self.assertRaises(FileExistsError): write_bundle(directory, source, graph)
            self.assertEqual(before, {p.name: p.read_bytes() for p in directory.iterdir()})
            for name in before:
                path = directory / name; path.write_text("{}")
                with self.assertRaises(ValueError): verify_bundle(directory, source, graph)
                path.write_bytes(before[name])
            link = Path(temp) / "alias"; link.symlink_to(directory)
            with self.assertRaises(ValueError): verify_bundle(link, source, graph)

    def test_interrupted_bundle_is_not_accepted(self):
        source, graph = self.materialized()
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp) / "bundle"
            original = json.dump
            calls = 0
            def interrupted(value, stream, **kwargs):
                nonlocal calls
                calls += 1
                if calls == 2: raise OSError("simulated write interruption")
                return original(value, stream, **kwargs)
            with patch("materialize_disjoint_musique.json.dump", side_effect=interrupted):
                with self.assertRaises(OSError): write_bundle(directory, source, graph)
            self.assertFalse((directory / "receipt.json").exists())
            with self.assertRaises((ValueError, json.JSONDecodeError)):
                verify_bundle(directory, source, graph)
            with self.assertRaises(FileExistsError): write_bundle(directory, source, graph)

    def test_graph_source_binding_is_required_before_write(self):
        source, graph = self.materialized(); graph["source_sha256"] = "wrong"
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp) / "bundle"
            with self.assertRaisesRegex(ValueError, "another source"):
                write_bundle(directory, source, graph)
            self.assertFalse(directory.exists())


if __name__ == "__main__":
    unittest.main()
