"""Bind the 2Wiki development run to its exact corpus, vectors and role allocation."""
import copy
import hashlib
import json
from pathlib import Path

from evaluate_retrieval import canonical, digest

PROTOCOL = Path(__file__).resolve().parents[1] / 'benchmarks/retrieval/twowiki-strength-development-v1.json'

CAPTURED_PROTOCOL = PROTOCOL.with_name('twowiki-captured1536-development-v1.json')

ROOT = Path(__file__).resolve().parents[1]
RESERVED_PROTOCOL = PROTOCOL.with_name('twowiki-captured1536-reserved-v1.json')
RESERVED_DATASET = (
    '2WikiMultiHopQA corrected public development: 120 reserved confirmation queries; '
    '40 development queries excluded from measured results'
)
RESERVED_SCOPE = (
    'Reserved confirmation retrieval evaluation after captured1536 development. '
    'No default admission, official hidden-test, model-unseen or agent-improvement claim.'
)



def verify_development_evidence(expected):
    """Require exact published development evidence and unchanged comparison inputs."""
    reference = expected['development_evidence']
    path = (ROOT / reference['report']).resolve()
    if not path.is_relative_to(ROOT.resolve()) or not path.is_file():
        raise ValueError('Development evidence must be a repository artifact')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('Development evidence digest differs')
    report = json.loads(raw)
    if (report.get('status') != 'completed' or report.get('evaluation_stage') != 'development'
        or report.get('failures') != [] or report.get('verified_fences_unchanged') is not True
        or report.get('verified_current_sources_and_relations') is not True):
        raise ValueError('Development evidence is not a completed verified run')
    prior = report['plan']['protocol']
    if report['plan']['protocol_sha256'] != digest(prior):
        raise ValueError('Development protocol identity differs')
    if prior.get('protocol_version') != 'twowiki_captured1536_development_v1':
        raise ValueError('Confirmation requires the captured1536 development protocol')
    # Compare every field, including future fields, except the explicit role transition and its fixed audience labels.
    transitioned = copy.deepcopy(prior)
    transitioned.update(
        protocol_version='twowiki_captured1536_reserved_v1', split='test',
        dataset=RESERVED_DATASET, scope_of_evidence=RESERVED_SCOPE,
        measured_query_ids=prior['reserved_query_ids'],
        development_query_ids=prior['measured_query_ids'],
        development_evidence=reference,
    )
    transitioned.pop('reserved_query_ids')
    if canonical(transitioned) != canonical(expected):
        raise ValueError('Reserved comparison changes a frozen development input')
    if (sorted(prior['reserved_query_ids']) != expected['measured_query_ids']
        or sorted(prior['measured_query_ids']) != expected['development_query_ids']
        or sorted(query['id'] for query in report['queries']) != expected['development_query_ids']
        or set(expected['measured_query_ids']) & set(expected['development_query_ids'])):
        raise ValueError('Reserved and development roles differ or overlap')


def verify_twowiki_protocol(protocol, fixture, graph=None):
    reserved = protocol.get('protocol_version') == 'twowiki_captured1536_reserved_v1'
    captured = reserved or protocol.get('protocol_version') == 'twowiki_captured1536_development_v1'
    path = RESERVED_PROTOCOL if reserved else (CAPTURED_PROTOCOL if captured else PROTOCOL)
    if reserved and not path.is_file():
        raise ValueError('Reserved protocol is not frozen; reserved results must remain unopened')
    expected = json.loads(path.read_text())
    if canonical(protocol) != canonical(expected):
        raise ValueError('2Wiki comparison differs from its frozen protocol')
    if reserved:
        verify_development_evidence(expected)
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
    roles = [('development', 'development_query_ids'), ('test', 'measured_query_ids')] if reserved else [('development', 'measured_query_ids'), ('test', 'reserved_query_ids')]
    for split, key in roles:
        if sorted(q['id'] for q in fixture['queries'] if q['split'] == split) != expected[key]:
            raise ValueError('2Wiki role allocation differs from the frozen cohort')
    if (fixture['provenance']['selection_manifest_sha256'] != expected['selection_manifest_sha256']
        or fixture['provenance']['split_mapping'] != expected['split_mapping']
        or len(fixture['documents']) != expected['documents']):
        raise ValueError('2Wiki selection provenance differs')
    if graph is not None and digest(graph) != expected['graph_sha256']:
        raise ValueError('2Wiki graph differs from the frozen document construction')
