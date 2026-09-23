#!/usr/bin/env python3
"""Materialize a replayed 2Wiki cohort and document-only graph with bundle receipts."""
import argparse
import copy
import json
import os
from pathlib import Path

import audit_twowiki_source as twowiki
from build_embedding_fixture import validate_source
from evaluate_retrieval import digest
from import_musique_graph import GRAPH_POLICY, _document_graph
import select_cross_source_cohort as selection

POLICY = {**copy.deepcopy(GRAPH_POLICY), 'version': 'twowiki_document_title_graph_v1',
          'inputs': ['id', 'title', 'sentences'],
          'rendering': 'Sentences joined in source order with one ASCII space; title excluded from mention text',
          'identity': 'twowiki- prefix and SHA256 of canonical title/sentences object'}
SPLITS = {'development': 'development', 'confirmation': 'test'}
SPLIT_MEANINGS = {'development': 'Selected development role from public development source',
                  'test': 'Reserved confirmation role from public development source; not official hidden test'}


def document_graph(documents):
    if any(set(doc) != {'id', 'title', 'sentences'} for doc in documents):
        raise ValueError('Graph accepts document fields only')
    verified = {}
    paragraphs = []
    for doc in documents:
        expected = twowiki.document_projection([[doc['title'], doc['sentences']]])[0]
        if expected != doc or doc['id'] in verified:
            raise ValueError('Duplicate or altered document identity')
        paragraph = {'id': doc['id'], 'title': doc['title'], 'paragraph_text': ' '.join(doc['sentences'])}
        verified[doc['id']] = paragraph
        paragraphs.append(paragraph)
    relations, metadata = _document_graph(paragraphs, POLICY, lambda doc: verified.get(doc['id']) == doc)
    metadata['sentence_projection_sha256'] = digest(sorted(documents, key=lambda d: d['id']))
    return relations, metadata


def build_selected(manifest, rows):
    selection.validate_plan(manifest['plan'])
    if set(manifest['selected_ids']) != set(SPLITS):
        raise ValueError('Both selected roles are required')
    index = {'twowiki:' + row['_id']: row for row in rows}
    if len(index) != len(rows):
        raise ValueError('Duplicate official source identity')
    documents, queries, observed = {}, [], set()
    for role in manifest['plan']['role_order']:
        categories = manifest['selected_ids'][role]
        if set(categories) != set(twowiki.TYPES):
            raise ValueError('Incomplete selected categories')
        for category in manifest['plan']['category_order']:
            ids = categories[category]
            if len(ids) != manifest['plan']['quotas'][role][category]:
                raise ValueError('Selected quota differs from the frozen plan')
            for qid in ids:
                if qid in observed or qid not in index:
                    raise ValueError('Repeated or missing selected identity')
                observed.add(qid)
                row = index[qid]
                if row['type'] != category or twowiki.issues(row):
                    raise ValueError('Selected source category or structure differs')
                projected = twowiki.document_projection(row['context'])
                titles = {doc['title']: doc for doc in projected}
                for doc in projected:
                    if doc['id'] in documents and documents[doc['id']] != doc:
                        raise ValueError('Document identity collision')
                    documents[doc['id']] = doc
                evidence = []
                for title, position in row['supporting_facts']:
                    doc = titles[title]
                    evidence.append({'document_id': doc['id'], 'sentence_index': position,
                                     'sentence_sha256': digest(doc['sentences'][position])})
                queries.append({'id': qid, 'text': row['question'], 'split': SPLITS[role],
                                'category': category, 'judgments': {e['document_id']: 1 for e in evidence},
                                'judgments_complete': False,
                                'support_sentence_evidence': sorted(evidence, key=lambda e: (e['document_id'], e['sentence_index']))})
    if len(observed) != manifest['selected_queries']:
        raise ValueError('Selected query count differs')
    projection = sorted(documents.values(), key=lambda doc: doc['id'])
    relations, graph = document_graph(projection)
    source = {'schema_version': 1, 'kind': 'authorized_relevance',
              'provenance': {'dataset': '2WikiMultiHopQA corrected public development source',
                             'source': 'https://github.com/Alab-NII/2wikimultihop',
                             'paper': 'https://aclanthology.org/2020.coling-main.580/',
                             'selection_manifest_sha256': digest(manifest), 'sources': manifest['sources'],
                             'split_mapping': SPLIT_MEANINGS,
                             'corpus': 'Deduplicated union of every selected context document, including distractors',
                             'judgments': 'Grade 1 for documents containing source supporting sentences; all other merged-corpus documents unjudged',
                             'selection_limits': manifest['limits']},
              'documents': [{'id': doc['id'], 'text': doc['title'] + '\n' + ' '.join(doc['sentences'])} for doc in projection],
              'queries': sorted(queries, key=lambda query: query['id'])}
    validate_source(source)
    graph.update(schema_version=1, source_sha256=digest(source), projection=projection, relations=relations)
    return source, graph


def materialize(manifest, archive, musique_sources, prior_bytes):
    replay = selection.build(archive, musique_sources, prior_bytes, manifest['plan'])
    if digest(replay) != digest(manifest):
        raise ValueError('Selection must replay exactly before materialization')
    return build_selected(manifest, twowiki.read_source(archive))


def receipt(source, graph):
    if graph['source_sha256'] != digest(source):
        raise ValueError('Graph belongs to another corpus')
    return {'schema_version': 1, 'status': 'materialized_not_evaluated',
            'source_sha256': digest(source), 'graph_sha256': digest(graph),
            'selection_manifest_sha256': source['provenance']['selection_manifest_sha256'],
            'documents': len(source['documents']), 'relations': len(graph['relations']),
            'queries_by_split': {split: sum(q['split'] == split for q in source['queries']) for split in SPLITS.values()},
            'split_mapping': SPLIT_MEANINGS, 'graph_policy_sha256': graph['policy_sha256'],
            'graph_inputs': 'Document ID, title and source sentences only; no questions, answers, support labels or evidence triples',
            'embedding_generation_performed': False, 'retrieval_performed': False}


def write_bundle(directory, source, graph):
    expected = {'source.json': source, 'relations.json': graph, 'receipt.json': receipt(source, graph)}
    directory.mkdir()
    for name, value in expected.items():
        with (directory / name).open('x') as stream:
            json.dump(value, stream, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())


def verify_bundle(directory, source, graph):
    if directory.is_symlink():
        raise ValueError('Bundle cannot be a symlink')
    expected = {'source.json': source, 'relations.json': graph, 'receipt.json': receipt(source, graph)}
    for name, value in expected.items():
        path = directory / name
        if path.is_symlink() or not path.is_file() or digest(json.loads(path.read_text())) != digest(value):
            raise ValueError('Incomplete or altered corpus bundle: ' + name)
    return expected['receipt.json']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('materialize', 'verify'))
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--musique-source-dir', type=Path, required=True)
    parser.add_argument('--prior-fixture', type=Path, action='append', required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--bundle', type=Path, required=True)
    args = parser.parse_args()
    if args.action == 'materialize' and (args.bundle.exists() or args.bundle.is_symlink()):
        parser.error('Preserve existing bundles; choose a new directory')
    source, graph = materialize(json.loads(args.manifest.read_text()), args.archive,
        {name: (args.musique_source_dir / name).read_bytes() for name in selection.SOURCE_HASHES},
        [path.read_bytes() for path in args.prior_fixture])
    if args.action == 'materialize':
        write_bundle(args.bundle, source, graph)
    print(json.dumps(verify_bundle(args.bundle, source, graph)))


if __name__ == '__main__':
    main()
