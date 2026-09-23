#!/usr/bin/env python3
"""Reject drift in the frozen development fixture, ranking protocol and role split."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from evaluate_retrieval import digest
from graph_retrieval_checks import proof_fixture
from evaluate_graph_retrieval import evaluation_stage, evaluation_queries, request_for
from export_graph_evaluation import export
from twowiki_evaluation_contract import PROTOCOL, verify_twowiki_protocol


def synthetic_contract():
    fixture, graph, state, _, _ = proof_fixture()
    fixture['provenance'] = {'selection_manifest_sha256': 'synthetic-selection',
                             'split_mapping': {'development': 'synthetic development', 'test': 'synthetic reserved'}}
    protocol = json.loads(PROTOCOL.read_text())
    source = copy.deepcopy(fixture)
    source.pop('space'); source.pop('embedding_provenance')
    for group in ('documents', 'queries'):
        for row in source[group]: row.pop('vector')
    protocol.update(fixture_sha256=digest(fixture), embedding_space=fixture['space'], source_sha256=digest(source),
                    graph_sha256=digest(graph), documents=len(fixture['documents']),
                    measured_query_ids=sorted(q['id'] for q in fixture['queries'] if q['split']=='development'),
                    reserved_query_ids=sorted(q['id'] for q in fixture['queries'] if q['split']=='test'),
                    selection_manifest_sha256='synthetic-selection', split_mapping=fixture['provenance']['split_mapping'])
    return fixture, graph, state, protocol


class DevelopmentContractChecks(unittest.TestCase):
    def test_development_only_and_unchanged_strength_seed(self):
        fixture, _, state, protocol = synthetic_contract()
        self.assertEqual(evaluation_stage(protocol), 'development')
        self.assertEqual(set(evaluation_queries(fixture, protocol)), set(protocol['measured_query_ids']))
        self.assertTrue(set(evaluation_queries(fixture, protocol)).isdisjoint(protocol['reserved_query_ids']))
        route, body = request_for('graph_hybrid_strength', fixture['queries'][0], fixture, state, protocol)
        self.assertEqual(body['query']['ranking_version'], 'typed_path_strength_v1')
        self.assertEqual(body['query']['seed']['ranking_version'], 'weighted_rrf_v1')
        with self.assertRaises(ValueError): evaluation_stage(dict(protocol, split='test'))

    def test_canonical_protocol_rejects_rehashed_overrides(self):
        fixture, graph, _, protocol = synthetic_contract()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'protocol.json'; path.write_text(json.dumps(protocol))
            with patch('twowiki_evaluation_contract.PROTOCOL', path):
                verify_twowiki_protocol(protocol, fixture, graph)
                for key, value in [('split','test'),('scan_limit',9999),('repetitions',1),
                                   ('graph_seed_hybrid_version','weighted_rrf_v2'),('extra',True)]:
                    changed = dict(protocol, **{key:value})
                    with self.assertRaisesRegex(ValueError, 'frozen protocol'):
                        verify_twowiki_protocol(changed, fixture)
                    report = {'status':'completed','failures':[], 'verified_fences_unchanged':True,
                              'verified_current_sources_and_relations':True,
                              'plan':{'fixture_sha256':digest(fixture),'protocol':changed,'protocol_sha256':digest(changed)}}
                    with self.assertRaises(ValueError): export(report,fixture)

    def test_fixture_vectors_source_and_provenance_cannot_drift(self):
        fixture, graph, _, protocol = synthetic_contract()
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'protocol.json'; path.write_text(json.dumps(protocol))
            with patch('twowiki_evaluation_contract.PROTOCOL',path):
                for mutate in (lambda f:f['documents'][0]['vector'].__setitem__(0,99),
                               lambda f:f['queries'][0].update(split='test'),
                               lambda f:f['documents'][0].update(text='changed'),
                               lambda f:f['space'].update(revision='changed'),
                               lambda f:f['provenance'].update(selection_manifest_sha256='changed')):
                    changed=copy.deepcopy(fixture); mutate(changed)
                    with self.assertRaises(ValueError):verify_twowiki_protocol(protocol,changed,graph)
                changed=copy.deepcopy(graph);changed['source_sha256']='changed'
                with self.assertRaisesRegex(ValueError,'graph differs'):verify_twowiki_protocol(protocol,fixture,changed)

    def test_checked_in_protocol_preserves_agreed_arms_and_cardinality(self):
        protocol=json.loads(PROTOCOL.read_text())
        self.assertEqual(len(protocol['measured_query_ids']),40)
        self.assertEqual(len(protocol['reserved_query_ids']),120)
        self.assertEqual(len(set(protocol['measured_query_ids']+protocol['reserved_query_ids'])),160)
        self.assertEqual(len(protocol['methods']),7)
        self.assertEqual(protocol['repetitions'],3)
        self.assertFalse(protocol['default_admission'])
        self.assertEqual(protocol['primary_comparison'],{'candidate':'graph_hybrid_strength','baseline':'weighted_rrf_v1'})


if __name__=='__main__':unittest.main()
