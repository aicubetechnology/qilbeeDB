#!/usr/bin/env python3
"""Verify source-bound materialization, annotation separation and recovery."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import materialize_twowiki_cohort as materializer
from audit_twowiki_source import document_projection
from cross_source_selection_checks import fixture, plan
import select_cross_source_cohort as selection


def bundle():
    rows = fixture()
    manifest = selection.select_rows(rows, [], {}, plan())
    manifest['sources'] = {'synthetic': True}
    return rows, manifest, materializer.build_selected(manifest, rows)


class MaterializationChecks(unittest.TestCase):
    def test_roles_documents_and_support_sentence_provenance(self):
        rows, manifest, (source, graph) = bundle()
        self.assertEqual(materializer.receipt(source, graph)['queries_by_split'], {'development': 4, 'test': 4})
        self.assertEqual({q['id'] for q in source['queries']}, {qid for categories in manifest['selected_ids'].values() for ids in categories.values() for qid in ids})
        self.assertTrue(all(doc['id'].startswith('twowiki-') for doc in source['documents']))
        self.assertTrue(all(not q['judgments_complete'] for q in source['queries']))
        self.assertTrue(all(set(q['judgments']) == {e['document_id'] for e in q['support_sentence_evidence']} for q in source['queries']))
        self.assertFalse(materializer.receipt(source, graph)['retrieval_performed'])

    def test_graph_rejects_labels_and_altered_sentence_identity(self):
        docs = document_projection([['Alpha Group', ['Beta Group provides evidence.']], ['Beta Group', ['Alpha Group is related.']]])
        relations, report = materializer.document_graph(docs)
        self.assertGreater(len(relations), 0)
        self.assertEqual((relations, report), materializer.document_graph(list(reversed(docs))))
        bad = copy.deepcopy(docs)
        bad[0]['supporting'] = True
        with self.assertRaisesRegex(ValueError, 'document fields'):
            materializer.document_graph(bad)
        bad = copy.deepcopy(docs)
        bad[0]['sentences'].append('Altered')
        with self.assertRaisesRegex(ValueError, 'identity'):
            materializer.document_graph(bad)
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            materializer.document_graph(docs + [docs[0]])

    def test_changed_questions_answers_and_gold_triples_do_not_change_relations(self):
        rows, manifest, (source, graph) = bundle()
        changed = copy.deepcopy(rows)
        for row in changed:
            row.update(question=row['question'] + ' changed', answer='Different answer',
                       evidences=[['gold subject', 'gold relation', 'gold object']])
        altered_source, altered_graph = materializer.build_selected(manifest, changed)
        self.assertNotEqual(source, altered_source)
        self.assertEqual(graph['relations'], altered_graph['relations'])
        self.assertEqual(graph['sentence_projection_sha256'], altered_graph['sentence_projection_sha256'])

    def test_retrieval_verifier_accepts_exact_graph_and_rejects_altered_policy(self):
        from graph_evaluation_contract import verify_fixture_graph
        _, _, (source, graph) = bundle()
        fixture = copy.deepcopy(source)
        fixture.update(space={'provider': 'synthetic', 'model': 'test', 'revision': '1', 'dimensions': 1},
                       embedding_provenance='Synthetic vectors only')
        for group in ('documents', 'queries'):
            for entry in fixture[group]:
                entry['vector'] = [1.0]
        verify_fixture_graph(fixture, graph)
        for mutate in (lambda g: g['policy'].update(version='unknown'),
                       lambda g: g['policy'].update(mention_targets_per_document=999),
                       lambda g: g['projection'][0]['sentences'].append('tampered')):
            changed = copy.deepcopy(graph)
            mutate(changed)
            with self.assertRaises(ValueError):
                verify_fixture_graph(fixture, changed)

    def test_manifest_replay_precedes_source_materialization(self):
        rows, manifest, _ = bundle()
        changed = copy.deepcopy(manifest)
        changed['selected_queries'] += 1
        with patch.object(selection, 'build', return_value=manifest), patch.object(materializer.twowiki, 'read_source') as reader:
            with self.assertRaisesRegex(ValueError, 'replay exactly'):
                materializer.materialize(changed, None, {}, [])
            reader.assert_not_called()

    def test_duplicate_roles_missing_ids_and_wrong_categories_fail(self):
        rows, manifest, _ = bundle()
        for change in (lambda m: m['selected_ids']['confirmation']['comparison'].append('missing'),
                       lambda m: m['selected_ids']['confirmation']['comparison'].__setitem__(0, 'missing'),
                       lambda m: m['selected_ids']['confirmation']['comparison'].__setitem__(0, m['selected_ids']['development']['comparison'][0]),
                       lambda m: m['selected_ids']['confirmation']['comparison'].__setitem__(0, m['selected_ids']['confirmation']['inference'][0])):
            bad = copy.deepcopy(manifest)
            change(bad)
            with self.assertRaises(ValueError):
                materializer.build_selected(bad, rows)

    def test_bundle_rejects_missing_altered_symlinked_and_overwritten_files(self):
        _, _, (source, graph) = bundle()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'bundle'
            materializer.write_bundle(path, source, graph)
            materializer.verify_bundle(path, source, graph)
            with self.assertRaises(FileExistsError):
                materializer.write_bundle(path, source, graph)
            receipt = path / 'receipt.json'
            original = receipt.read_text()
            receipt.unlink()
            with self.assertRaisesRegex(ValueError, 'Incomplete'):
                materializer.verify_bundle(path, source, graph)
            receipt.write_text(original)
            changed = json.loads(original)
            changed['retrieval_performed'] = True
            receipt.write_text(json.dumps(changed))
            with self.assertRaisesRegex(ValueError, 'altered'):
                materializer.verify_bundle(path, source, graph)
            outside = Path(directory) / 'external.json'
            outside.write_text(original)
            receipt.unlink()
            receipt.symlink_to(outside)
            with self.assertRaisesRegex(ValueError, 'altered'):
                materializer.verify_bundle(path, source, graph)


if __name__ == '__main__':
    unittest.main()
