#!/usr/bin/env python3
"""Checks for quarantine precedence, capacity accounting and replay integrity."""
import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import audit_cross_source_overlap as audit
from cross_source_overlap_checks import twowiki_row
from disjoint_selection_checks import row as musique_row


class CapacityChecks(unittest.TestCase):
    def test_known_missing_alias_is_quarantined_before_conflicts(self):
        prior = musique_row(2, 100)
        row = twowiki_row()
        row['answer'] = prior['answer']
        report = audit.audit_rows([row], [prior], {'Q1': {'place'}})
        self.assertEqual(len(report['known_missing_alias_quarantine']), 1)
        self.assertEqual(report['nonexclusive_exclusions'], {})
        self.assertEqual(sum(report['eligible_by_category'].values()), 0)
        self.assertEqual(report['annotation_coverage']['projected_records'], 1)

    def test_structural_and_alias_quarantines_are_separate(self):
        bad = twowiki_row()
        bad['context'].append(copy.deepcopy(bad['context'][0]))
        report = audit.audit_rows([bad], [musique_row(2, 100)], {})
        self.assertEqual(report['structural_quarantined_records'], 1)
        self.assertEqual(report['known_missing_alias_quarantine'], [])
        self.assertEqual(report['annotation_coverage'], {})

    def test_missing_unprovided_ids_remain_explicit_without_invented_aliases(self):
        row = twowiki_row()
        row.update(answer_id=None, evidences_id=[])
        report = audit.audit_rows([row], [musique_row(2, 100)], {})
        self.assertEqual(report['eligible_by_category']['compositional'], 1)
        self.assertEqual(report['annotation_coverage']['missing_answer_id'], 1)
        self.assertEqual(report['annotation_coverage']['without_evidence_id_triples'], 1)
        self.assertFalse(report['retrieval_performed'])
        self.assertEqual(report['selected_queries'], 0)

    def test_category_conflicts_and_history_projection_are_deterministic(self):
        prior = musique_row(2, 100)
        row = twowiki_row()
        row.update(answer_id=None, evidences_id=[], answer=prior['answer'])
        report = audit.audit_rows([row], [prior], {})
        self.assertEqual(report['exclusions_by_category']['compositional'], {'answer': 1})
        self.assertEqual(report, audit.audit_rows([row], [prior, copy.deepcopy(prior)], {}))
        changed = copy.deepcopy(prior)
        changed['answer'] = 'Different historical answer'
        self.assertNotEqual(report['history_projection_sha256'], audit.audit_rows([row], [changed], {})['history_projection_sha256'])

    def test_cli_never_overwrites_and_replays_all_fields(self):
        expected = audit.audit_rows([twowiki_row()], [musique_row(2, 100)], {'Q1': {'place'}, 'Q2': {'entity'}})
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in audit.SOURCE_HASHES:
                (root / name).write_bytes(b'fixture')
            (root / 'prior.json').write_bytes(b'fixture')
            output = root / 'result.json'
            def run(action):
                args = ['audit', action, '--archive', 'unused', '--musique-source-dir', str(root), '--prior-fixture', str(root / 'prior.json'), '--output', str(output)]
                with patch('sys.argv', args), patch.object(audit, 'build', return_value=expected), contextlib.redirect_stdout(io.StringIO()):
                    audit.main()
            run('audit')
            original = output.read_bytes()
            run('verify')
            with self.assertRaises(FileExistsError):
                run('audit')
            self.assertEqual(original, output.read_bytes())
            for field, value in [('selected_queries', 1), ('retrieval_performed', True), ('history_projection_sha256', 'altered')]:
                changed = copy.deepcopy(expected)
                changed[field] = value
                output.write_text(json.dumps(changed))
                with self.assertRaisesRegex(ValueError, 'does not replay'):
                    run('verify')


if __name__ == '__main__':
    unittest.main()
