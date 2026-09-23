"""Bind the 2Wiki development run to its exact corpus, vectors and role allocation."""
import copy
import json
from pathlib import Path

from evaluate_retrieval import canonical, digest

PROTOCOL = Path(__file__).resolve().parents[1] / 'benchmarks/retrieval/twowiki-strength-development-v1.json'

CAPTURED_PROTOCOL = PROTOCOL.with_name('twowiki-captured1536-development-v1.json')

def verify_twowiki_protocol(protocol, fixture, graph=None):
    captured = protocol.get('protocol_version') == 'twowiki_captured1536_development_v1'
    expected = json.loads((CAPTURED_PROTOCOL if captured else PROTOCOL).read_text())
    if canonical(protocol) != canonical(expected):
        raise ValueError('2Wiki comparison differs from its frozen protocol')
    if digest(fixture) != expected['fixture_sha256'] or fixture['space'] != expected['embedding_space']:
        raise ValueError('2Wiki vectors or embedding identity differ from the frozen fixture')
    if captured:
        provenance = fixture['embedding_provenance']
        if (provenance.get('kind') != 'captured-vector-set'
            or provenance.get('manifest_sha256') != expected['captured_manifest_sha256']
            or provenance.get('offline_only') is not True
            or provenance.get('newly_generated_queries_compatible') is not False
            or provenance.get('provider_weight_revision') is not None):
            raise ValueError('Captured vector identity or offline boundary differs')
    source = copy.deepcopy(fixture)
    source.pop('space')
    source.pop('embedding_provenance')
    for group in ('documents', 'queries'):
        for row in source[group]:
            row.pop('vector')
    if digest(source) != expected['source_sha256']:
        raise ValueError('2Wiki source differs from the frozen corpus')
    for split, key in [('development', 'measured_query_ids'), ('test', 'reserved_query_ids')]:
        if sorted(q['id'] for q in fixture['queries'] if q['split'] == split) != expected[key]:
            raise ValueError('2Wiki role allocation differs from the frozen cohort')
    if (fixture['provenance']['selection_manifest_sha256'] != expected['selection_manifest_sha256']
        or fixture['provenance']['split_mapping'] != expected['split_mapping']
        or len(fixture['documents']) != expected['documents']):
        raise ValueError('2Wiki selection provenance differs')
    if graph is not None and digest(graph) != expected['graph_sha256']:
        raise ValueError('2Wiki graph differs from the frozen document construction')
