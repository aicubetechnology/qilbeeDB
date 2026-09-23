"""Reject incomplete numeric timing evidence before starting an HTTP experiment."""
import copy
import unittest
from unittest.mock import Mock, patch
from evaluate_retrieval import digest
from evaluate_graph_retrieval import evaluate, validate_generation_timings
from twowiki_evaluation_checks import synthetic_contract


class GenerationPreflightChecks(unittest.TestCase):
    def setUp(self):
        self.fixture, self.graph, self.state, self.protocol = synthetic_contract()
        self.state['fixture_sha256'] = digest(self.fixture)
        self.generation = {'fixture_sha256': digest(self.fixture), 'queries': {
            q['id']: {'elapsed_ms': 2.5} for q in self.fixture['queries'] if q['split']=='development'}}

    def test_measured_roles_only(self):
        validate_generation_timings(self.generation, self.fixture, self.protocol)
        self.assertTrue(all(q['id'] not in self.generation['queries'] for q in self.fixture['queries'] if q['split']=='test'))

    def test_invalid_timing_never_reaches_http(self):
        query_id = next(iter(self.generation['queries']))
        for invalid in (None, True, -1, '2', float('nan'), float('inf'), 10**1000):
            candidate = copy.deepcopy(self.generation)
            candidate['queries'][query_id]['elapsed_ms'] = invalid
            client = Mock()
            with patch('evaluate_graph_retrieval.verify_protocol'), patch('evaluate_graph_retrieval.validate_state'):
                with self.assertRaisesRegex(ValueError, 'finite nonnegative'):
                    evaluate(client, self.fixture, self.graph, self.state, self.protocol,
                             None, None, candidate)
            client.call.assert_not_called()

    def test_missing_and_wrong_fixture_fail_before_http(self):
        candidates = [dict(self.generation, queries={}),
                      {'fixture_sha256':digest(self.fixture), 'batches':[{'elapsed_ms':3}]},
                      dict(self.generation, fixture_sha256='different')]
        for candidate in candidates:
            client = Mock()
            with patch('evaluate_graph_retrieval.verify_protocol'), patch('evaluate_graph_retrieval.validate_state'):
                with self.assertRaises(ValueError):
                    evaluate(client, self.fixture, self.graph, self.state, self.protocol,
                             None, None, candidate)
            client.call.assert_not_called()

    def test_lexical_does_not_require_unused_generation_timings(self):
        protocol = dict(self.protocol, methods=['lexical'])
        validate_generation_timings({'fixture_sha256':digest(self.fixture)}, self.fixture, protocol)


if __name__ == '__main__':
    unittest.main()
