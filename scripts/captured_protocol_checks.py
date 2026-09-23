"""Preserve query roles, input identity and methods when changing external vectors."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from evaluate_retrieval import digest
from evaluate_graph_retrieval import evaluation_stage, evaluation_queries
from twowiki_evaluation_checks import synthetic_contract
from twowiki_evaluation_contract import PROTOCOL, CAPTURED_PROTOCOL, verify_twowiki_protocol


class CapturedProtocolChecks(unittest.TestCase):
    def test_only_embedding_evidence_and_protocol_identity_change(self):
        old = json.loads(PROTOCOL.read_text())
        new = json.loads(CAPTURED_PROTOCOL.read_text())
        changed = {'protocol_version', 'fixture_sha256', 'embedding_space'}
        for key in set(old) - changed:
            self.assertEqual(old[key], new[key], key)
        self.assertEqual(new['embedding_space']['dimensions'], 1536)
        self.assertEqual(new['embedding_space']['revision'], 'captured-' + new['captured_manifest_sha256'][:24])
        self.assertEqual(evaluation_stage(new), 'development')
        self.assertFalse(new['default_admission'])
        self.assertNotEqual(old['fixture_sha256'], new['fixture_sha256'])

    def test_captured_identity_is_bound_and_reserved_queries_excluded(self):
        fixture, graph, _, protocol = synthetic_contract()
        fixture['embedding_provenance'] = {'kind':'captured-vector-set', 'manifest_sha256':'manifest',
            'offline_only':True,'newly_generated_queries_compatible':False,'provider_weight_revision':None}
        protocol.update(protocol_version='twowiki_captured1536_development_v1',
                        captured_manifest_sha256='manifest', fixture_sha256=digest(fixture))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'protocol.json'
            path.write_text(json.dumps(protocol))
            with patch('twowiki_evaluation_contract.CAPTURED_PROTOCOL', path):
                verify_twowiki_protocol(protocol, fixture, graph)
                self.assertEqual(set(evaluation_queries(fixture, protocol)), set(protocol['measured_query_ids']))
                self.assertTrue(set(evaluation_queries(fixture, protocol)).isdisjoint(protocol['reserved_query_ids']))
                for key, value in [('manifest_sha256','other'), ('offline_only',False),
                                   ('newly_generated_queries_compatible',True), ('provider_weight_revision','invented')]:
                    changed = copy.deepcopy(fixture)
                    changed['embedding_provenance'][key] = value
                    # Even a coherently repinned fixture cannot relax the snapshot boundary.
                    updated = dict(protocol, fixture_sha256=digest(changed))
                    path.write_text(json.dumps(updated))
                    with self.assertRaisesRegex(ValueError, 'offline boundary'):
                        verify_twowiki_protocol(updated, changed)
                path.write_text(json.dumps(protocol))
                with self.assertRaisesRegex(ValueError, 'frozen protocol'):
                    verify_twowiki_protocol(dict(protocol, split='test'), fixture)

    def test_unknown_version_cannot_alias_captured_contract(self):
        captured = json.loads(CAPTURED_PROTOCOL.read_text())
        with self.assertRaises(ValueError):
            evaluation_stage(dict(captured, protocol_version='unregistered_version'))


if __name__ == '__main__':
    unittest.main()
