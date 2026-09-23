#!/usr/bin/env python3
"""Project explicit overlap identities; never produce retrieval inputs or edges."""
import hashlib
import json
import re

from audit_twowiki_source import issues
from select_disjoint_musique import normalized, overlap_keys
from evaluate_retrieval import digest

ALIAS_SHA256 = 'f08ffcb6c2cefca9bdbe86b4248d6ad7a7743762d3f7264c14ff0bae85726fb6'
POLICY = 'musique_twowiki_overlap_projection_v1'


def aliases_from_bytes(raw):
    if hashlib.sha256(raw).hexdigest() != ALIAS_SHA256:
        raise ValueError('Official alias member digest differs')
    result = {}
    for line in raw.splitlines():
        entry = json.loads(line)
        if set(entry) != {'Q_id', 'aliases', 'demonyms'}:
            raise ValueError('Unexpected alias schema')
        qid = entry['Q_id']
        if not isinstance(qid, str) or not re.fullmatch(r'Q\d+', qid) or qid in result:
            raise ValueError('Invalid or duplicate alias identity')
        values = set()
        for field in ('aliases', 'demonyms'):
            if not isinstance(entry[field], list):
                raise ValueError('Alias values must be lists')
            values.update(normalized(value) for value in entry[field])
        result[qid] = values
    if not result:
        raise ValueError('Empty alias source')
    return result


def empty_keys():
    return {k: set() for k in ('query_id', 'question', 'answer', 'annotated_entity', 'support_title', 'support_text')}


def add_support(keys, title, text):
    keys['support_title'].add(normalized(title))
    keys['support_text'].add(digest(normalized(text)))


def musique_projection(row):
    # Reuse the established integrity checks for decomposition references.
    overlap_keys(row)
    keys = empty_keys()
    keys['query_id'].add('musique:' + row['id'])
    keys['question'].add(normalized(row['question']))
    keys['answer'].update(normalized(value) for value in [row['answer'], *row.get('answer_aliases', [])])
    paragraphs = {p['idx']: p for p in row['paragraphs']}
    for step in row['question_decomposition']:
        keys['question'].add(normalized(step['question']))
        keys['answer'].add(normalized(step['answer']))
        paragraph = paragraphs[step['paragraph_support_idx']]
        add_support(keys, paragraph['title'], paragraph['paragraph_text'])
    return keys


def twowiki_projection(row, aliases):
    failures = issues(row)
    if failures:
        raise ValueError('Quarantined source record: ' + ', '.join(failures))
    keys = empty_keys()
    keys['query_id'].add('twowiki:' + row['_id'])
    keys['question'].add(normalized(row['question']))
    keys['answer'].add(normalized(row['answer']))
    evidence = row.get('evidences')
    evidence_ids = row.get('evidences_id')
    if not isinstance(evidence, list) or not isinstance(evidence_ids, list):
        raise ValueError('Declared evidence arrays are required for overlap projection')
    for triple in evidence:
        if not isinstance(triple, list) or len(triple) != 3:
            raise ValueError('Malformed evidence triple')
        for cell in triple:
            normalized(cell)
        keys['annotated_entity'].update(normalized(triple[i]) for i in (0, 2))
    qids = set()
    endpoint_ids = set()
    answer_id = row.get('answer_id')
    if answer_id is not None:
        if not isinstance(answer_id, str) or not re.fullmatch(r'Q\d+', answer_id):
            raise ValueError('Invalid answer entity identity')
        qids.add(answer_id)
    for triple in evidence_ids:
        if not isinstance(triple, list) or len(triple) != 3:
            raise ValueError('Malformed evidence identity triple')
        for cell in triple:
            normalized(cell)
        if not re.fullmatch(r'Q\d+', triple[0]):
            raise ValueError('Invalid evidence subject identity')
        for i in (0, 2):
            if re.fullmatch(r'Q\d+', triple[i]):
                qids.add(triple[i])
                endpoint_ids.add(triple[i])
            else:
                keys['annotated_entity'].add(normalized(triple[i]))
    missing = sorted(qids - aliases.keys())
    if answer_id is not None:
        keys['answer'].update(aliases.get(answer_id, set()))
    for qid in endpoint_ids:
        keys['annotated_entity'].update(aliases.get(qid, set()))
    context = dict(row['context'])
    for title in {support[0] for support in row['supporting_facts']}:
        add_support(keys, title, ' '.join(context[title]))
    coverage = {'answer_id_present': answer_id is not None,
                'evidence_triples': len(evidence), 'evidence_id_triples': len(evidence_ids),
                'referenced_alias_ids': len(qids), 'missing_alias_ids': missing,
                'complete_alias_coverage_claimed': False}
    return keys, coverage


def merge_history(projections):
    union = empty_keys()
    for projection in projections:
        if set(projection) != set(union):
            raise ValueError('Overlap projection categories differ')
        for key, values in projection.items():
            if not isinstance(values, set) or any(not isinstance(v, str) or not v for v in values):
                raise ValueError('Invalid overlap identity set')
            union[key].update(values)
    return union


def conflicts(candidate, history):
    checked = merge_history([candidate])
    prior = merge_history([history])
    return conflict_categories(checked, prior)


def conflict_categories(candidate, history):
    """Compare prevalidated projections; preserve the candidate annotation role."""
    prior_entities = history['answer'] | history['annotated_entity']
    return sorted(key for key in candidate if candidate[key] &
                  (prior_entities if key in ('answer', 'annotated_entity') else history[key]))
