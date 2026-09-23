#!/usr/bin/env python3
"""Audit pinned 2WikiMultiHopQA public development data before cohort design.

No selection, retrieval, embeddings, answer-derived edges or leaderboard claim.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import zipfile

from evaluate_retrieval import digest

ARCHIVE_SHA256 = '95df2bf56fdabe034e27aebc580e02264232203cf52552f9efe8a919e5529eef'
DEVELOPMENT_SHA256 = '79f77ae104088ea8e25b1a65dbece768d45771194663bc5660ec9a98070dadf5'
TYPES = ('bridge_comparison', 'comparison', 'compositional', 'inference')
VERSION = 'twowiki_public_development_source_audit_v1'


def text(value):
    return isinstance(value, str) and bool(value.strip())


def issues(row):
    """Return structural quarantine reasons; never repair or choose a gold title."""
    reasons = set()
    if not text(row.get('_id')) or row.get('type') not in TYPES:
        reasons.add('invalid_identity_or_category')
    if not text(row.get('question')) or not text(row.get('answer')):
        reasons.add('missing_question_or_answer')
    context = row.get('context')
    if not isinstance(context, list) or not context:
        return sorted(reasons | {'invalid_context'})
    titles = {}
    for document in context:
        if (not isinstance(document, list) or len(document) != 2 or not text(document[0])
                or not isinstance(document[1], list) or not document[1]
                or not all(text(s) for s in document[1])):
            reasons.add('invalid_context')
            continue
        title, sentences = document
        if title in titles:
            reasons.add('duplicate_context_title')
        else:
            titles[title] = sentences
    supports = row.get('supporting_facts')
    if not isinstance(supports, list) or not supports:
        reasons.add('missing_support_labels')
    else:
        seen = set()
        for support in supports:
            if (not isinstance(support, list) or len(support) != 2 or not text(support[0])
                    or type(support[1]) is not int):
                reasons.add('invalid_support_label')
                continue
            title, position = support
            if (title, position) in seen:
                reasons.add('duplicate_support_label')
            seen.add((title, position))
            if title not in titles:
                reasons.add('missing_support_title')
            elif position < 0 or position >= len(titles[title]):
                reasons.add('support_sentence_out_of_range')
    return sorted(reasons)


def document_projection(context):
    """Only title/sentence pairs enter this boundary; no question or labels."""
    if not isinstance(context, list):
        raise ValueError('Context must contain document pairs only')
    documents = []
    titles = set()
    for entry in context:
        if (not isinstance(entry, list) or len(entry) != 2 or not text(entry[0])
                or not isinstance(entry[1], list) or not entry[1]
                or not all(text(sentence) for sentence in entry[1])):
            raise ValueError('Invalid document-only projection')
        title, sentences = entry
        if title in titles:
            raise ValueError('Ambiguous duplicate context title')
        titles.add(title)
        documents.append({'id': 'twowiki-' + digest({'title': title, 'sentences': sentences}),
                          'title': title, 'sentences': list(sentences)})
    if not documents:
        raise ValueError('Empty document projection')
    return sorted(documents, key=lambda d: d['id'])


def audit_rows(rows):
    if not isinstance(rows, list) or not rows:
        raise ValueError('Source must contain records')
    if any(not isinstance(r, dict) or not text(r.get('_id')) for r in rows):
        raise ValueError('Invalid source record identity')
    if len({r['_id'] for r in rows}) != len(rows):
        raise ValueError('Duplicate source query ID')
    categories = Counter()
    accepted = Counter()
    reasons = Counter()
    quarantined = []
    for row in rows:
        categories[row.get('type', 'unknown')] += 1
        failures = issues(row)
        if failures:
            reasons.update(failures)
            quarantined.append({'id': row['_id'], 'reasons': failures})
        else:
            document_projection(row['context'])
            accepted[row['type']] += 1
    return {'audit_version': VERSION, 'source_split': 'public_development',
            'archive_sha256': ARCHIVE_SHA256, 'development_sha256': DEVELOPMENT_SHA256,
            'source_records': len(rows), 'source_categories': dict(sorted(categories.items())),
            'structurally_eligible_categories': {k: accepted[k] for k in TYPES},
            'quarantine_reasons': dict(sorted(reasons.items())),
            'quarantined_records': sorted(quarantined, key=lambda r: r['id']),
            'selected_queries': 0, 'retrieval_performed': False,
            'limits': ['Structural eligibility does not establish disjointness from observed history',
                       'Source categories are not interchangeable with MuSiQue hop strata',
                       'Quarantine reasons are nonexclusive; ambiguous titles are not silently resolved',
                       'Sentence labels do not fully judge relevance in a merged retrieval corpus',
                       'Public data may occur in model training; no hidden-test or reasoning-gain claim']}


def read_source(path):
    hasher = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(block)
    if hasher.hexdigest() != ARCHIVE_SHA256:
        raise ValueError('Official archive digest differs')
    with zipfile.ZipFile(path) as archive:
        if archive.namelist().count('dev.json') != 1:
            raise ValueError('Ambiguous development member')
        raw = archive.read('dev.json')
    if hashlib.sha256(raw).hexdigest() != DEVELOPMENT_SHA256:
        raise ValueError('Official development digest differs')
    return json.loads(raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('audit', 'verify'))
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = audit_rows(read_source(args.archive))
    if args.action == 'verify':
        if digest(json.loads(args.output.read_text())) != digest(report):
            raise ValueError('Source audit does not replay exactly')
    else:
        with args.output.open('x') as stream:
            json.dump(report, stream, indent=2)
            stream.write('\n')
    print(json.dumps({'audit_sha256': digest(report), 'records': report['source_records'],
                      'quarantined': len(report['quarantined_records'])}))


if __name__ == '__main__':
    main()
