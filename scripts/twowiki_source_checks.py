#!/usr/bin/env python3
"""Synthetic boundary tests for public multi-step source auditing."""
import copy
import contextlib
import io
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import audit_twowiki_source as source


def fixture():
    return {'_id': 'example', 'type': 'compositional', 'question': 'Which place?',
            'answer': 'Place', 'context': [['First', ['Sentence one.']], ['Second', ['Sentence two.']]],
            'supporting_facts': [['First', 0], ['Second', 0]]}


class SourceChecks(unittest.TestCase):
    def test_duplicate_title_is_quarantined_even_with_identical_text(self):
        row = fixture()
        row['context'].append(copy.deepcopy(row['context'][0]))
        self.assertIn('duplicate_context_title', source.issues(row))
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            source.document_projection(row['context'])
        report = source.audit_rows([row])
        self.assertEqual(len(report['quarantined_records']), 1)
        self.assertEqual(report['structurally_eligible_categories']['compositional'], 0)

    def test_support_indices_and_missing_titles_are_not_repaired(self):
        for label, reason in ((['First', 1], 'support_sentence_out_of_range'),
                              (['First', -1], 'support_sentence_out_of_range'),
                              (['Absent', 0], 'missing_support_title'),
                              (['First', True], 'invalid_support_label')):
            row = fixture()
            row['supporting_facts'] = [label]
            self.assertIn(reason, source.issues(row))

    def test_document_projection_cannot_depend_on_labels_or_question(self):
        row = fixture()
        expected = source.document_projection(row['context'])
        row.update(question='Changed question', answer='Changed answer',
                   evidences=[['gold', 'relation', 'target']], supporting_facts=[['Second', 0]])
        self.assertEqual(expected, source.document_projection(row['context']))
        with self.assertRaises(ValueError):
            source.document_projection(row)
        changed = copy.deepcopy(row['context'])
        changed[0][1][0] = 'Updated sentence.'
        self.assertNotEqual(expected, source.document_projection(changed))
        self.assertEqual(expected, source.document_projection(list(reversed(row['context']))))

    def test_duplicate_identity_fails_and_counts_do_not_double_count_quarantine(self):
        row = fixture()
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            source.audit_rows([row, copy.deepcopy(row)])
        row['context'].append(copy.deepcopy(row['context'][0]))
        row['supporting_facts'].append(['Missing', 5])
        report = source.audit_rows([row])
        self.assertEqual(len(report['quarantined_records']), 1)
        self.assertEqual(sum(report['quarantine_reasons'].values()), 2)
        self.assertFalse(report['retrieval_performed'])
        self.assertEqual(report['selected_queries'], 0)

    def test_cli_replay_rejects_tampering_and_preserves_existing_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'audit.json'
            def run(action):
                with patch('sys.argv', ['audit', action, '--archive', 'unused.zip', '--output', str(output)]), patch.object(source, 'read_source', return_value=[fixture()]), contextlib.redirect_stdout(io.StringIO()):
                    source.main()
            run('audit')
            original = output.read_bytes()
            run('verify')
            with self.assertRaises(FileExistsError):
                run('audit')
            self.assertEqual(original, output.read_bytes())
            changed = json.loads(original)
            changed['selected_queries'] = 1
            output.write_text(json.dumps(changed))
            with self.assertRaisesRegex(ValueError, 'does not replay'):
                run('verify')

    def test_archive_and_member_bytes_are_bound_without_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'source.zip'
            raw = json.dumps([fixture()]).encode()
            with zipfile.ZipFile(path, 'w') as archive:
                archive.writestr('dev.json', raw)
            sha = hashlib.sha256(path.read_bytes()).hexdigest()
            with patch.object(source, 'ARCHIVE_SHA256', sha), patch.object(source, 'DEVELOPMENT_SHA256', hashlib.sha256(raw).hexdigest()):
                self.assertEqual(source.read_source(path), [fixture()])
                with patch.object(source, 'DEVELOPMENT_SHA256', 'wrong'):
                    with self.assertRaisesRegex(ValueError, 'development digest'):
                        source.read_source(path)
                path.write_bytes(path.read_bytes() + b'changed')
                with self.assertRaisesRegex(ValueError, 'archive digest'):
                    source.read_source(path)


if __name__ == '__main__':
    unittest.main()
