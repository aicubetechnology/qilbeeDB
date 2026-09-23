"""Bind the 2Wiki development run to its exact corpus, vectors and role allocation."""
import copy
import json
from pathlib import Path

from evaluate_retrieval import canonical, digest

PROTOCOL = Path(__file__).resolve().parents[1] / 'benchmarks/retrieval/twowiki-strength-development-v1.json'


def verify_twowiki_protocol(protocol, fixture, graph=None):
    expected = json.loads(PROTOCOL.read_text())
    if canonical(protocol) != canonical(expected):
        raise ValueError('2Wiki comparison differs from its frozen protocol')
    if digest(fixture) != expected['fixture_sha256'] or fixture['space'] != expected['embedding_space']:
        raise ValueError('2Wiki vectors or embedding identity differ from the frozen fixture')
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
