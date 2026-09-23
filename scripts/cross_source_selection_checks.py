#!/usr/bin/env python3
"""Synthetic selection checks; no real shared cohort is activated."""
import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import cross_source_overlap as overlap
import select_cross_source_cohort as selection
from disjoint_selection_checks import row as musique_row


def fixture():
    rows = []
    for category in selection.source.TYPES:
        for i in range(4):
            uid = category + '-' + str(i)
            rows.append({'_id': uid, 'type': category, 'question': 'Question ' + uid,
                         'answer': 'Answer ' + uid, 'answer_id': None,
                         'context': [['Title ' + uid, ['Text ' + uid]]],
                         'supporting_facts': [['Title ' + uid, 0]],
                         'evidences': [['Subject ' + uid, 'relation', 'Answer ' + uid]],
                         'evidences_id': []})
    return rows


def plan():
    return {'seed': 'synthetic-frozen', 'role_order': ['confirmation', 'development'],
            'category_order': list(selection.source.TYPES),
            'quotas': {role: {category: 1 for category in selection.source.TYPES}
                       for role in selection.ROLES}}


class SelectionChecks(unittest.TestCase):
    def test_deterministic_order_without_quality_and_mutual_disjointness(self):
        rows = fixture()
        result = selection.select_rows(rows, [], {}, plan())
        reverse = copy.deepcopy(list(reversed(rows)))
        for i, row in enumerate(reverse):
            row['retrieval_score'] = i * 99
        self.assertEqual(result, selection.select_rows(reverse, [], {}, plan()))
        index = {'twowiki:' + row['_id']: row for row in rows}
        used = overlap.empty_keys()
        count = 0
        for categories in result['selected_ids'].values():
            for ids in categories.values():
                for qid in ids:
                    projection, _ = overlap.twowiki_projection(index[qid], {})
                    self.assertEqual(overlap.conflicts(projection, used), [])
                    used = overlap.merge_history([used, projection])
                    count += 1
        self.assertEqual(count, 8)
        self.assertFalse(result['retrieval_results_used'])

    def test_development_cannot_reuse_confirmation_answer_as_entity(self):
        rows = fixture()
        for row in rows:
            row['evidences'][0][0] = 'Shared annotated entity'
        with self.assertRaisesRegex(ValueError, 'no quota was relaxed'):
            selection.select_rows(rows, [], {}, plan())

    def test_history_alias_quarantine_and_support_exclusions(self):
        rows = fixture()
        prior = musique_row(2, 100)
        blocked = 'twowiki:' + rows[0]['_id']
        rows[0]['question'] = prior['question_decomposition'][0]['question']
        missing = 'twowiki:' + rows[1]['_id']
        rows[1]['answer_id'] = 'Q98765'
        result = selection.select_rows(rows, [prior], {}, plan())
        ids = {qid for categories in result['selected_ids'].values() for ids in categories.values() for qid in ids}
        self.assertNotIn(blocked, ids)
        self.assertNotIn(missing, ids)

    def test_plan_requires_explicit_complete_orders_and_integer_quotas(self):
        for mutate in (lambda p: p.update(seed=''),
                       lambda p: p.update(role_order=['development', 'development']),
                       lambda p: p.update(category_order=list(reversed(p['category_order']))[:-1]),
                       lambda p: p['quotas']['development'].update(comparison=True),
                       lambda p: p['quotas']['confirmation'].update(comparison=0),
                       lambda p: p.update(automatic_retry=True)):
            bad = plan()
            mutate(bad)
            with self.assertRaises(ValueError):
                selection.select_rows(fixture(), [], {}, bad)

    def test_insufficient_quota_fails_without_partial_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / 'selection.json'
            source_plan = root / 'plan.json'
            settings = plan()
            settings['quotas']['confirmation']['comparison'] = 99
            source_plan.write_text(json.dumps(settings))
            for name in selection.SOURCE_HASHES:
                (root / name).write_bytes(b'fixture')
            (root / 'prior').write_bytes(b'fixture')
            args = ['select', 'select', '--archive', 'unused', '--musique-source-dir', str(root), '--prior-fixture', str(root / 'prior'), '--plan', str(source_plan), '--output', str(output)]
            with patch('sys.argv', args), patch.object(selection, 'build', side_effect=lambda a, s, h, p: selection.select_rows(fixture(), [], {}, p)):
                with self.assertRaisesRegex(ValueError, 'no quota was relaxed'):
                    selection.main()
            self.assertFalse(output.exists())

    def test_cli_replay_binds_roles_ids_and_refuses_overwrite(self):
        result = selection.select_rows(fixture(), [], {}, plan())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / 'result.json'
            settings = root / 'plan.json'
            settings.write_text(json.dumps(plan()))
            for name in selection.SOURCE_HASHES:
                (root / name).write_bytes(b'fixture')
            (root / 'prior').write_bytes(b'fixture')
            def run(action):
                args = ['selector', action, '--archive', 'unused', '--musique-source-dir', str(root), '--prior-fixture', str(root / 'prior'), '--output', str(output)]
                if action == 'select': args += ['--plan', str(settings)]
                with patch('sys.argv', args), patch.object(selection, 'build', return_value=result), contextlib.redirect_stdout(io.StringIO()):
                    selection.main()
            run('select')
            original = output.read_bytes()
            run('verify')
            with self.assertRaises(FileExistsError): run('select')
            self.assertEqual(original, output.read_bytes())
            for mutate in (lambda r: r.update(retrieval_results_used=True),
                           lambda r: r['selected_ids']['confirmation']['comparison'].append('invented'),
                           lambda r: r.update(combined_projection_sha256='changed')):
                changed = copy.deepcopy(result)
                mutate(changed)
                output.write_text(json.dumps(changed))
                with self.assertRaisesRegex(ValueError, 'does not replay'):
                    run('verify')


if __name__ == '__main__':
    unittest.main()
