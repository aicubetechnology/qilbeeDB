#!/usr/bin/env python3
"""Synthetic checks for cross-source leakage exclusion identities."""
import copy
import hashlib
import json
import unittest
from unittest.mock import patch

import cross_source_overlap as overlap
from disjoint_selection_checks import row as musique_row
from twowiki_source_checks import fixture


def twowiki_row():
    row = fixture()
    row.update(answer_id='Q1', evidences=[['Entity', 'relation', 'Place']],
               evidences_id=[['Q2', 'relation', 'Q1']])
    return row


class OverlapChecks(unittest.TestCase):
    def test_aliases_and_demonyms_are_normalized_and_hashed(self):
        raw = json.dumps({'Q_id': 'Q1', 'aliases': ['ＰＬＡＣＥ'], 'demonyms': ['Resident']}).encode()
        with patch.object(overlap, 'ALIAS_SHA256', hashlib.sha256(raw).hexdigest()):
            aliases = overlap.aliases_from_bytes(raw)
            self.assertEqual(aliases, {'Q1': {'place', 'resident'}})
            with self.assertRaisesRegex(ValueError, 'digest'):
                overlap.aliases_from_bytes(raw + b'\n')
        duplicate = raw + b'\n' + raw
        with patch.object(overlap, 'ALIAS_SHA256', hashlib.sha256(duplicate).hexdigest()):
            with self.assertRaisesRegex(ValueError, 'duplicate'):
                overlap.aliases_from_bytes(duplicate)

    def test_cross_source_answer_alias_and_subquestion_exclusion(self):
        prior = musique_row(2, 100)
        current = twowiki_row()
        current['question'] = prior['question_decomposition'][0]['question'].upper()
        projection, _ = overlap.twowiki_projection(current, {'Q1': {overlap.normalized(prior['answer'])}})
        self.assertEqual(overlap.conflicts(projection, overlap.musique_projection(prior)), ['annotated_entity', 'answer', 'question'])

    def test_evidence_endpoint_conflicts_keep_their_annotation_role(self):
        prior = musique_row(2, 100)
        current = twowiki_row()
        current['evidences'][0][0] = prior['answer']
        projection, _ = overlap.twowiki_projection(current, {})
        history = overlap.musique_projection(prior)
        self.assertEqual(overlap.conflicts(projection, history), ['annotated_entity'])
        self.assertNotIn(overlap.normalized(prior['answer']), projection['answer'])
        self.assertEqual(overlap.conflicts(history, projection), ['answer'])

    def test_same_title_different_segmentation_still_conflicts(self):
        prior = musique_row(2, 100)
        current = twowiki_row()
        current['context'][0][0] = prior['paragraphs'][0]['title']
        current['supporting_facts'][0][0] = prior['paragraphs'][0]['title']
        projection, _ = overlap.twowiki_projection(current, {})
        self.assertEqual(overlap.conflicts(projection, overlap.musique_projection(prior)), ['support_title'])

    def test_missing_entity_annotations_never_imply_complete_alias_coverage(self):
        current = twowiki_row()
        _, coverage = overlap.twowiki_projection(current, {'Q1': {'place'}})
        self.assertEqual(coverage['missing_alias_ids'], ['Q2'])
        self.assertFalse(coverage['complete_alias_coverage_claimed'])
        current.update(answer_id=None, evidences_id=[])
        _, coverage = overlap.twowiki_projection(current, {})
        self.assertFalse(coverage['answer_id_present'])
        self.assertEqual(coverage['evidence_id_triples'], 0)
        self.assertFalse(coverage['complete_alias_coverage_claimed'])

    def test_query_ids_are_source_qualified_and_history_is_order_independent(self):
        prior = musique_row(2, 100)
        current = twowiki_row()
        current['_id'] = prior['id']
        first = overlap.musique_projection(prior)
        second, _ = overlap.twowiki_projection(current, {})
        self.assertEqual(overlap.conflicts(second, first), [])
        self.assertEqual(overlap.merge_history([first, second]), overlap.merge_history([second, first, first]))
        self.assertIn('query_id', overlap.conflicts(second, overlap.merge_history([first, second])))

    def test_invalid_support_and_evidence_fail_before_projection(self):
        current = twowiki_row()
        for change in (lambda r: r['supporting_facts'].append(['Missing', 0]),
                       lambda r: r.update(evidences=[['Only one cell']]),
                       lambda r: r.update(evidences_id=[['not-an-id', 'relation', 'Q1']]),
                       lambda r: r.update(answer_id=''),
                       lambda r: r.update(evidences=None)):
            bad = copy.deepcopy(current)
            change(bad)
            with self.assertRaises(ValueError):
                overlap.twowiki_projection(bad, {})


if __name__ == '__main__':
    unittest.main()
