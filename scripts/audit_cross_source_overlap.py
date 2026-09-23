#!/usr/bin/env python3
"""Replay cross-source eligibility before selecting a shared retrieval cohort."""
import argparse
from collections import Counter
import json
from pathlib import Path
import zipfile

import cross_source_overlap as overlap
import audit_twowiki_source as source
from evaluate_retrieval import digest
from select_disjoint_musique import SOURCE_HASHES, verified_inputs

VERSION = 'musique_twowiki_capacity_audit_v1'


def audit_rows(rows, history_rows, aliases):
    # Fail on duplicate identities and validate structural quarantine consistently.
    structural = source.audit_rows(rows)
    history = overlap.merge_history(overlap.musique_projection(row) for row in history_rows)
    eligible = Counter()
    excluded = Counter()
    excluded_by_category = {category: Counter() for category in source.TYPES}
    coverage = Counter()
    quarantine = []
    for row in rows:
        if source.issues(row):
            continue
        keys, observed = overlap.twowiki_projection(row, aliases)
        coverage['projected_records'] += 1
        coverage['missing_answer_id'] += not observed['answer_id_present']
        coverage['without_evidence_id_triples'] += observed['evidence_id_triples'] == 0
        if observed['missing_alias_ids']:
            quarantine.append({'id': row['_id'], 'missing_alias_ids': observed['missing_alias_ids']})
            continue
        reasons = overlap.conflict_categories(keys, history)
        if reasons:
            excluded.update(reasons)
            excluded_by_category[row['type']].update(reasons)
        else:
            eligible[row['type']] += 1
    return {'audit_version': VERSION, 'projection_policy': overlap.POLICY,
            'normalization': {'version': 'nfkc_casefold_unicode_words_v1',
                              'rule': 'NFKC casefold, Unicode word tokens joined by one space; punctuation-only text retains normalized literal'},
            'entity_annotation_rule': 'Evidence endpoints and their aliases are annotated entities, not presumed intermediate answers; each compares against historical answers and annotated entities',
            'source_records': len(rows), 'source_categories': structural['source_categories'],
            'structural_audit_sha256': digest(structural),
            'structural_quarantined_records': len(structural['quarantined_records']),
            'known_missing_alias_quarantine': sorted(quarantine, key=lambda item: item['id']),
            'previous_unique_queries': len({row['id'] for row in history_rows}),
            'history_projection_sha256': digest({key: sorted(values) for key, values in history.items()}),
            'eligible_by_category': {category: eligible[category] for category in source.TYPES},
            'nonexclusive_exclusions': dict(sorted(excluded.items())),
            'exclusions_by_category': {category: dict(sorted(counts.items())) for category, counts in excluded_by_category.items()},
            'annotation_coverage': dict(sorted(coverage.items())),
            'selected_queries': 0, 'retrieval_performed': False,
            'limits': ['Capacity is against declared history only; within-cohort overlaps can reduce selection',
                       'Known missing alias entries are quarantined before overlap counting',
                       'Absent source entity annotations and undeclared aliases remain coverage limitations',
                       'Title-level exclusions are stronger than exact paragraph equality and change distribution',
                       'Conflict categories are nonexclusive and cannot be summed as unique excluded records',
                       'Public source data and disjoint identities do not prove model-training exclusion or statistical independence']}


def build(archive, musique_sources, prior_bytes):
    rows = source.read_source(archive)
    with zipfile.ZipFile(archive) as stream:
        aliases = overlap.aliases_from_bytes(stream.read('id_aliases.json'))
    _, history, prior_hashes = verified_inputs(musique_sources, prior_bytes, 'train')
    result = audit_rows(rows, history, aliases)
    result['sources'] = {'twowiki_archive_sha256': source.ARCHIVE_SHA256,
                         'twowiki_development_sha256': source.DEVELOPMENT_SHA256,
                         'twowiki_aliases_sha256': overlap.ALIAS_SHA256,
                         'musique_source_sha256': dict(SOURCE_HASHES),
                         'prior_fixture_sha256': prior_hashes}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('audit', 'verify'))
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--musique-source-dir', type=Path, required=True)
    parser.add_argument('--prior-fixture', type=Path, action='append', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = build(args.archive, {name: (args.musique_source_dir / name).read_bytes() for name in SOURCE_HASHES},
                   [path.read_bytes() for path in args.prior_fixture])
    if args.action == 'verify':
        if digest(json.loads(args.output.read_text())) != digest(result):
            raise ValueError('Cross-source capacity audit does not replay exactly')
    else:
        with args.output.open('x') as stream:
            json.dump(result, stream, indent=2)
            stream.write('\n')
    print(json.dumps({'audit_sha256': digest(result), 'eligible_by_category': result['eligible_by_category']}))


if __name__ == '__main__':
    main()
