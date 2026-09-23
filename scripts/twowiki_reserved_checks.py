#!/usr/bin/env python3
"""Reserved-run gates tested offline; no reserved HTTP queries or model calls."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import twowiki_evaluation_contract as contract
from evaluate_retrieval import digest
from evaluate_graph_retrieval import evaluation_stage, evaluate, evaluation_queries
from twowiki_evaluation_checks import synthetic_contract

REPORT = Path(__file__).resolve().parents[1] / 'benchmarks/retrieval/twowiki-captured1536-development-report.json'


def expected_protocol():
    report = json.loads(REPORT.read_text())
    prior = report['plan']['protocol']
    expected = copy.deepcopy(prior)
    expected.update(protocol_version='twowiki_captured1536_reserved_v1', split='test',
                    dataset=contract.RESERVED_DATASET, scope_of_evidence=contract.RESERVED_SCOPE,
                    measured_query_ids=prior['reserved_query_ids'],
                    development_query_ids=prior['measured_query_ids'],
                    development_evidence={'report': str(REPORT.relative_to(contract.ROOT)),
                                          'sha256': hashlib.sha256(REPORT.read_bytes()).hexdigest()})
    expected.pop('reserved_query_ids')
    return expected, report


class ReservedGates(unittest.TestCase):
    def test_published_development_evidence_matches_unchanged_candidate(self):
        expected, _ = expected_protocol()
        contract.verify_development_evidence(expected)
        self.assertEqual(evaluation_stage(expected), 'reserved_comparison')
        self.assertEqual(len(expected['measured_query_ids']), 120)
        self.assertEqual(len(expected['development_query_ids']), 40)

    def test_model_formula_budget_and_cohort_changes_are_not_silent(self):
        expected, _ = expected_protocol()
        for mutate in (lambda p:p['embedding_space'].update(dimensions=3072),
                       lambda p:p.update(fixture_sha256='different-vectors'),
                       lambda p:p['graph_profiles'].update(graph_hybrid_strength='typed_path_balanced_v1'),
                       lambda p:p.update(scan_limit=9999),
                       lambda p:p['measured_query_ids'].__setitem__(0,p['development_query_ids'][0]),
                       lambda p:p.update(repetitions=4),
                       lambda p:p.update(seed=p['seed']+1),
                       lambda p:p.update(generation_sha256='changed'),
                       lambda p:p.update(query_timing_status='numeric'),
                       lambda p:p.update(default_admission=True),
                       lambda p:p.update(scope_of_evidence='Development-only results'),
                       lambda p:p.update(dataset='Official hidden test'),
                       lambda p:p.update(undeclared_future_setting=True),
                       lambda p:p.update(primary_comparison={'candidate':'graph_hybrid_balanced','baseline':'weighted_rrf_v1'})):
            changed = copy.deepcopy(expected)
            mutate(changed)
            with self.assertRaises(ValueError):
                contract.verify_development_evidence(changed)

    def test_modified_report_is_rejected_even_when_plausible(self):
        expected, report = expected_protocol()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root/'report.json'
            path.write_bytes(REPORT.read_bytes())
            expected['development_evidence']['report'] = 'report.json'
            with patch.object(contract, 'ROOT', root):
                contract.verify_development_evidence(expected)
                report['summary']['graph_hybrid_strength']['ndcg_at_10'] = 1.0
                path.write_text(json.dumps(report))
                with self.assertRaisesRegex(ValueError, 'digest'):
                    contract.verify_development_evidence(expected)

    def test_incomplete_run_or_relabelled_results_fail_with_updated_file_hash(self):
        expected, original = expected_protocol()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); path = root/'report.json'
            expected['development_evidence']['report'] = 'report.json'
            for mutate in (lambda r:r.update(status='failed'),
                           lambda r:r.update(evaluation_stage='reserved_comparison'),
                           lambda r:r.update(verified_fences_unchanged=False),
                           lambda r:r['queries'][0].update(id=expected['measured_query_ids'][0]),
                           lambda r:r['plan'].update(protocol_sha256='changed')):
                report = copy.deepcopy(original); mutate(report)
                raw = json.dumps(report).encode(); path.write_bytes(raw)
                expected['development_evidence']['sha256'] = hashlib.sha256(raw).hexdigest()
                with patch.object(contract, 'ROOT', root), self.assertRaises(ValueError):
                    contract.verify_development_evidence(expected)

    def test_missing_reserved_protocol_fails_before_fixture_use(self):
        expected, _ = expected_protocol()
        with tempfile.TemporaryDirectory() as directory, patch.object(contract, 'RESERVED_PROTOCOL', Path(directory)/'not-frozen.json'):
            with self.assertRaisesRegex(ValueError, 'not frozen'):
                contract.verify_twowiki_protocol(expected, {})

    def test_unfrozen_reserved_run_never_reaches_http(self):
        fixture, graph, state, protocol = synthetic_contract()
        protocol, _ = expected_protocol()
        graph.update(source_sha256=protocol['source_sha256'],
                     relations_sha256=protocol['relations_sha256'],
                     policy_sha256=protocol['graph_policy_sha256'])
        client = Mock()
        with tempfile.TemporaryDirectory() as directory, patch.object(
            contract, 'RESERVED_PROTOCOL', Path(directory)/'missing.json'
        ), patch('evaluate_graph_retrieval.verify_fixture_graph'):
            with self.assertRaisesRegex(ValueError, 'not frozen'):
                evaluate(client, fixture, graph, state, protocol, None, None, {})
        client.call.assert_not_called()

    def test_reserved_selection_excludes_every_development_query(self):
        protocol, report = expected_protocol()
        fixture = {'queries': [
            {'id': qid, 'split': split}
            for split, ids in [('test', protocol['measured_query_ids']),
                               ('development', protocol['development_query_ids'])]
            for qid in ids
        ]}
        measured = evaluation_queries(fixture, protocol)
        self.assertEqual(set(measured), set(protocol['measured_query_ids']))
        self.assertTrue(set(measured).isdisjoint(q['id'] for q in report['queries']))

    def test_external_report_path_is_not_a_repository_reference(self):
        expected, _ = expected_protocol()
        expected['development_evidence']['report'] = '../outside.json'
        with self.assertRaisesRegex(ValueError, 'repository artifact'):
            contract.verify_development_evidence(expected)


if __name__ == '__main__':
    unittest.main()
