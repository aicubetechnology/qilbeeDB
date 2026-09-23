#!/usr/bin/env python3
"""Freeze a category-stratified cohort without retrieval or quality-based selection."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

import audit_cross_source_overlap as capacity
import audit_twowiki_source as source
import cross_source_overlap as overlap
from evaluate_retrieval import digest
from select_disjoint_musique import SOURCE_HASHES, verified_inputs

VERSION = 'twowiki_cross_source_greedy_selection_v1'
ROLES = {'development', 'confirmation'}


def validate_plan(plan):
    if not isinstance(plan, dict) or set(plan) != {'seed', 'role_order', 'category_order', 'quotas'}:
        raise ValueError('Plan must explicitly specify seed, both orders and quotas')
    if not isinstance(plan['seed'], str) or not plan['seed'].strip():
        raise ValueError('Selection seed must be explicit and nonempty')
    for key, expected in (('role_order', ROLES), ('category_order', set(source.TYPES))):
        order = plan[key]
        if not isinstance(order, list) or not all(isinstance(item, str) for item in order) or len(order) != len(expected) or set(order) != expected:
            raise ValueError('Plan must list every role/category exactly once')
    if not isinstance(plan['quotas'], dict) or set(plan['quotas']) != ROLES:
        raise ValueError('Both role quotas are required')
    for quotas in plan['quotas'].values():
        if not isinstance(quotas, dict) or set(quotas) != set(source.TYPES) or any(type(n) is not int or n < 1 for n in quotas.values()):
            raise ValueError('Every category needs a positive integer quota')


def select_rows(rows, history_rows, aliases, plan):
    validate_plan(plan)
    audit = capacity.audit_rows(rows, history_rows, aliases)
    history = overlap.merge_history(overlap.musique_projection(row) for row in history_rows)
    candidates = {category: [] for category in source.TYPES}
    for row in rows:
        if source.issues(row):
            continue
        projection, coverage = overlap.twowiki_projection(row, aliases)
        if coverage['missing_alias_ids'] or overlap.conflict_categories(projection, history):
            continue
        candidates[row['type']].append(('twowiki:' + row['_id'], projection))
    for entries in candidates.values():
        entries.sort(key=lambda item: (hashlib.sha256((plan['seed'] + '\n' + item[0]).encode()).hexdigest(), item[0]))
    used = overlap.merge_history([history])
    selected = {role: {category: [] for category in source.TYPES} for role in plan['role_order']}
    for role in plan['role_order']:
        for category in plan['category_order']:
            quota = plan['quotas'][role][category]
            for qid, projection in candidates[category]:
                if len(selected[role][category]) == quota:
                    break
                if not overlap.conflict_categories(projection, used):
                    selected[role][category].append(qid)
                    for key, values in projection.items():
                        used[key].update(values)
            if len(selected[role][category]) != quota:
                raise ValueError(f'Insufficient disjoint {role}/{category} rows: requested {quota}, selected {len(selected[role][category])}; no quota was relaxed')
    return {'selection_version': VERSION, 'plan': json.loads(json.dumps(plan)),
            'ordering': 'Explicit role order, then category order, then SHA256(seed + newline + source-qualified query ID), then source-qualified query ID',
            'capacity_audit_sha256': digest(audit), 'selected_ids': selected,
            'selected_queries': sum(sum(len(ids) for ids in categories.values()) for categories in selected.values()),
            'combined_projection_sha256': digest({key: sorted(values) for key, values in used.items()}),
            'retrieval_results_used': False,
            'limits': ['Greedy exclusion selection changes distribution and does not prove statistical independence',
                       'Confirmation is a declared role, not a hidden official test split or evidence of model-training exclusion',
                       'Source entity/alias gaps and unlisted observation history remain limitations',
                       'Questions, answers and support labels serve exclusions only; no graph is constructed',
                       'A failed quota requires an explicit new protocol, not an automatic seed retry']}


def build(archive, musique_sources, prior_bytes, plan):
    validate_plan(plan)
    rows = source.read_source(archive)
    with zipfile.ZipFile(archive) as stream:
        aliases = overlap.aliases_from_bytes(stream.read('id_aliases.json'))
    _, history, hashes = verified_inputs(musique_sources, prior_bytes, 'train')
    result = select_rows(rows, history, aliases, plan)
    result['sources'] = {'twowiki_archive_sha256': source.ARCHIVE_SHA256,
                         'twowiki_development_sha256': source.DEVELOPMENT_SHA256,
                         'twowiki_aliases_sha256': overlap.ALIAS_SHA256,
                         'musique_source_sha256': dict(SOURCE_HASHES),
                         'prior_fixture_sha256': hashes}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('select', 'verify'))
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--musique-source-dir', type=Path, required=True)
    parser.add_argument('--prior-fixture', type=Path, action='append', required=True)
    parser.add_argument('--plan', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.action == 'select' and args.plan is None:
        parser.error('Selection requires an explicit frozen plan')
    if args.action == 'verify' and args.plan is not None:
        parser.error('Verification replays the plan embedded in the selection manifest')
    original = json.loads(args.output.read_text()) if args.action == 'verify' else None
    plan = original['plan'] if original is not None else json.loads(args.plan.read_text())
    result = build(args.archive, {name: (args.musique_source_dir / name).read_bytes() for name in SOURCE_HASHES},
                   [path.read_bytes() for path in args.prior_fixture], plan)
    if original is not None:
        if digest(original) != digest(result):
            raise ValueError('Cross-source selection does not replay exactly')
    else:
        with args.output.open('x') as stream:
            json.dump(result, stream, indent=2)
            stream.write('\n')
    print(json.dumps({'manifest_sha256': digest(result), 'selected_queries': result['selected_queries']}))


if __name__ == '__main__':
    main()
