"""Adversarial checks for the offline captured-vector handoff boundary."""
import copy
import hashlib
import struct
import unittest
from pathlib import Path
from unittest.mock import patch

from verify_captured_vectors import digest, verify


def evidence():
    source = {'documents': [{'id': 'doc', 'text': 'Evidence'}],
              'queries': [{'id': 'query', 'text': 'Question', 'split': 'development'}]}
    fixture = copy.deepcopy(source)
    for group in source:
        fixture[group][0]['vector'] = [0.5, 0.25]
    plan = {'model': 'example', 'dimensions': 2, 'batch_size': 1,
            'source_sha256': digest(source), 'batches': []}
    for group in source:
        request = {'model': 'example', 'dimensions': 2, 'encoding_format': 'float',
                   'input': [source[group][0]['text']]}
        plan['batches'].append({'group': group, 'offset': 0, 'count': 1,
                               'input_ids': [source[group][0]['id']], 'request_sha256': digest(request)})
    files = {'plan.json': plan}
    manifest = {'kind': 'captured-vector-set', 'source_sha256': digest(source),
                'generation_plan_sha256': digest(plan), 'provider': 'example', 'model': 'example',
                'dimensions': 2, 'provider_weight_revision': None,
                'float32_database_roundtrip_verified': False, 'entries': []}
    receipts, measured = [], []
    for number, batch in enumerate(plan['batches']):
        row = fixture[batch['group']][0]
        capture = {'plan_sha256': digest(plan), 'request_sha256': batch['request_sha256'],
                   'http_status': 200, 'captured_at': '2026-01-01T00:00:01+00:00',
                   'body': {'model': 'example', 'usage': {'total_tokens': 3},
                            'data': [{'index': 0, 'embedding': row['vector']}]}}
        dispatch = {'plan_sha256': digest(plan), 'request_sha256': batch['request_sha256'],
                    'dispatched_at': '2026-01-01T00:00:00+00:00'}
        files[f'{number:04d}.capture.json'] = capture
        files[f'{number:04d}.dispatched.json'] = dispatch
        manifest['entries'].append({'batch': number, 'batch_index': 0, 'dimensions': 2,
            'float32_le_sha256': hashlib.sha256(struct.pack('<2f', *row['vector'])).hexdigest(),
            'input_utf8_sha256': hashlib.sha256(row['text'].encode()).hexdigest(),
            'ordinal': 0, 'provider_vector_json_sha256': digest(row['vector']),
            'role': batch['group'], 'source_id': row['id']})
        receipts.append({'batch': number, 'captured_at': capture['captured_at'],
            'dispatched_at': dispatch['dispatched_at'], 'http_status': 200,
            'monetary_cost_status': 'not_calculated', 'monetary_cost_usd': None,
            'request_sha256': batch['request_sha256'], 'requested_model': 'example',
            'response_capture_sha256': digest(capture), 'returned_model': 'example', 'total_tokens': 3})
        measured.append({'batch': number, 'capture_sha256': digest(capture), 'count': 1,
                         'group': batch['group'], 'offset': 0, 'tokens': 3})
    original = copy.deepcopy(fixture)
    original.update(space={}, embedding_provenance={})
    fixture['space'] = {'provider': 'example', 'model': 'example', 'dimensions': 2,
                        'revision': 'captured-' + digest(manifest)[:24]}
    fixture['embedding_provenance'] = {'kind': 'captured-vector-set', 'manifest_sha256': digest(manifest),
        'provider_weight_revision': None, 'offline_only': True, 'newly_generated_queries_compatible': False,
        'provider_drift_excluded': False, 'float32_database_roundtrip_verified': False,
        'raw_capture_fixture_sha256': digest(original)}
    audit = {'batches': receipts, 'total_tokens': 6, 'known_usage_batches': 2, 'unknown_usage_batches': 0,
        'evaluation_fixture_sha256': digest(fixture), 'manifest_sha256': digest(manifest),
        'original_capture_fixture_sha256': digest(original), 'space': fixture['space'],
        'monetary_cost_usd': None, 'monetary_cost_status': 'not_calculated'}
    files.update({'evaluation-fixture.json': fixture, 'fixture.json': original,
                  'vector-input-manifest.json': manifest, 'handoff-audit.json': audit,
                  'measurements.json': {'batches': measured, 'total_tokens': 6,
                                       'fixture_sha256': digest(original), 'plan_sha256': digest(plan)}})
    return source, files


class CapturedVectorChecks(unittest.TestCase):
    def setUp(self):
        self.source, self.files = evidence()
        self.fixture_hash = digest(self.files['evaluation-fixture.json'])
        self.manifest_hash = digest(self.files['vector-input-manifest.json'])

    def run_check(self):
        with patch('verify_captured_vectors.read', side_effect=lambda p: self.files[p.name]):
            return verify(Path('/unused'), self.source, self.fixture_hash, self.manifest_hash)

    def test_verified_snapshot_does_not_claim_database_or_weights_verification(self):
        result = self.run_check()
        self.assertEqual(result['verified_batches'], 2)
        self.assertEqual(result['total_tokens'], 6)
        self.assertFalse(result['float32_database_roundtrip_verified'])
        self.assertFalse(result['provider_weight_revision_verified'])
        self.assertIsNone(result['monetary_cost_usd'])

    def test_pinned_artifacts_reject_changed_vector(self):
        self.files['evaluation-fixture.json']['documents'][0]['vector'][0] = 0.8
        with self.assertRaisesRegex(ValueError, 'fixture digest'):
            self.run_check()

    def test_response_vector_must_match_frozen_fixture(self):
        self.files['0000.capture.json']['body']['data'][0]['embedding'] = [0.7, 0.2]
        with self.assertRaisesRegex(ValueError, 'Vector differs'):
            self.run_check()

    def test_response_indices_and_model(self):
        for field, value in [('index', 1), ('index', False)]:
            self.setUp()
            self.files['0000.capture.json']['body']['data'][0][field] = value
            with self.assertRaisesRegex(ValueError, 'response index'):
                self.run_check()
        self.setUp()
        self.files['0000.capture.json']['body']['model'] = 'another-model'
        with self.assertRaisesRegex(ValueError, 'Returned model'):
            self.run_check()

    def test_unknown_usage_and_cost_are_not_zero(self):
        self.files['0000.capture.json']['body']['usage']['total_tokens'] = None
        with self.assertRaisesRegex(ValueError, 'token usage'):
            self.run_check()
        self.setUp()
        self.files['handoff-audit.json']['monetary_cost_usd'] = 0
        with self.assertRaisesRegex(ValueError, 'cost'):
            self.run_check()

    def test_source_and_request_tampering(self):
        self.source['queries'][0]['text'] = 'Different question'
        with self.assertRaisesRegex(ValueError, 'Source identity'):
            self.run_check()
        self.setUp()
        self.files['0000.dispatched.json']['request_sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'Request content'):
            self.run_check()

    def test_receipts_bind_status_chronology_and_summary(self):
        self.files['0000.capture.json']['http_status'] = 429
        with self.assertRaisesRegex(ValueError, 'Unsuccessful'):
            self.run_check()
        self.setUp()
        self.files['0000.dispatched.json']['dispatched_at'] = '2026-01-02T00:00:00+00:00'
        with self.assertRaisesRegex(ValueError, 'chronology'):
            self.run_check()
        self.setUp()
        self.files['measurements.json']['batches'][0]['tokens'] = 0
        with self.assertRaisesRegex(ValueError, 'summary'):
            self.run_check()


if __name__ == '__main__':
    unittest.main()
