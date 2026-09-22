#!/usr/bin/env python3
"""Compare matched retrieval reports with one declared resource change."""
import argparse
import json
import math
import re
from pathlib import Path

from evaluate_retrieval import MODES, digest, save

FROZEN = ('fixture_sha256', 'manifest_sha256', 'scope_sha256', 'source_revisions',
          'model_space', 'trial_plan', 'conditions', 'server_health', 'metrics_definition')
ROW_FROZEN = ('split', 'category', 'ranking_version', 'ranked', 'metrics', 'counts',
              'ranking_profile', 'score_evidence')
AXES = ('cpu', 'memory', 'storage')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def percentile(values, fraction):
    ordered = sorted(values)
    index = (len(ordered) - 1) * fraction
    lower = int(index)
    upper = min(lower + 1, len(ordered) - 1)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (index - lower)


def validate_run(report, declaration):
    require(declaration['schema_version'] == 1, 'Unsupported hardware declaration')
    require(declaration['report_sha256'] == digest(report), 'Declaration does not bind this report')
    require(re.fullmatch(r'sha256:[0-9a-f]{64}', declaration['image_digest']), 'Immutable image digest required')
    for key in ('environment_id', 'storage', 'background_workload'):
        require(isinstance(declaration[key], str) and bool(declaration[key].strip()), 'Missing declared ' + key)
    require(report['valid_comparison'] is True and not report['failures'] and
            not any(report['violations'].values()), 'Invalid or incomplete retrieval report')
    require(report['trial_plan_sha256'] == digest(report['trial_plan']), 'Trial plan digest mismatch')
    resources = report['resources']
    snapshots = [resources['before'], resources['after']]
    for mode in MODES:
        measurement = resources['per_method_separate_warm_pass'][mode]
        snapshots.extend([measurement['before'], measurement['after']])
    first_identity = snapshots[0]['container_identity']
    require(bool(first_identity), 'Missing container identity')
    for sample in snapshots:
        require(sample['available'] is True and sample['stable_during_sampling'] is True,
                'Unavailable or unstable resource evidence')
        require(sample['container_identity'] == first_identity, 'Container restarted during the trial')
        metrics = sample['cgroup_v2']
        require('cpu.max' not in sample['unavailable_metrics'] and
                'memory.max' not in sample['unavailable_metrics'], 'Missing observed resource limits')
        require(metrics['cpu.max'] == declaration['cpu'] and
                metrics['memory.max'] == declaration['memory'], 'Declared limits differ from observations')
    rows = {}
    repetitions = report['trial_plan']['repetitions']
    require(type(repetitions) is int and repetitions > 0, 'Invalid repetitions')
    for row in report['rows']:
        key = (row['query_id'], row['mode'])
        require(key not in rows and row['mode'] in MODES, 'Duplicate or unknown query/method row')
        require(len(row['samples']) == repetitions, 'Missing latency samples')
        for sample in row['samples']:
            for field in ('retrieval_ms', 'http_ms', 'response_bytes'):
                value = sample[field]
                require(type(value) in (int, float) and math.isfinite(value) and value >= 0,
                        'Invalid latency or response size')
        rows[key] = row
    require(bool(rows), 'No query rows')
    for mode in MODES:
        declared_count = resources['per_method_separate_warm_pass'][mode]['queries']
        require(type(declared_count) is int and declared_count == sum(key[1] == mode for key in rows),
                'Query rows differ from recorded resource-pass coverage')
    for query in {key[0] for key in rows}:
        require(all((query, mode) in rows for mode in MODES), 'Incomplete method coverage')
    return rows


def compare(before, after, before_environment, after_environment):
    old = validate_run(before, before_environment)
    new = validate_run(after, after_environment)
    for key in FROZEN:
        require(before[key] == after[key], 'Frozen retrieval condition differs: ' + key)
    for key in ('image_digest', 'environment_id', 'background_workload'):
        require(before_environment[key] == after_environment[key], 'Environment differs: ' + key)
    changed = [axis for axis in AXES if before_environment[axis] != after_environment[axis]]
    require(len(changed) == 1, 'Exactly one resource axis must change')
    require(set(old) == set(new), 'Query coverage differs')
    paired = []
    for key in sorted(old):
        a, b = old[key], new[key]
        for field in ROW_FROZEN:
            require(a.get(field) == b.get(field), 'Ranking, coverage or evidence changed: ' + field)
        require(all(re.fullmatch(r'[0-9a-f]{64}', s['response_payload_sha256']) for s in a['samples'] + b['samples']), 'Missing untimed response digest')
        require(len({s['response_payload_sha256'] for s in a['samples'] + b['samples']}) == 1, 'Response content differs beyond timing')
        metrics = {}
        for field in ('retrieval_ms', 'http_ms'):
            aa, bb = [s[field] for s in a['samples']], [s[field] for s in b['samples']]
            metrics[field] = {'before_p50': percentile(aa, .5), 'after_p50': percentile(bb, .5),
                              'p50_delta': percentile(bb, .5) - percentile(aa, .5),
                              'before_p95': percentile(aa, .95), 'after_p95': percentile(bb, .95)}
        paired.append({'query_id': key[0], 'mode': key[1], 'latency': metrics})
    return {'schema_version': 1, 'changed_resource': changed[0],
            'before_report_sha256': digest(before), 'after_report_sha256': digest(after),
            'before_environment': before_environment, 'after_environment': after_environment,
            'per_query': paired,
            'response_comparison': 'Canonical JSON response digest excluding only top-level timing; raw response bytes remain in bound input reports.',
            'interpretation': 'Descriptive matched samples only. Environment, image and storage are operator declarations, not remote attestation. Low repetition counts cannot estimate tail latency reliably. These checks do not establish causality, production capacity, relevance improvement or agent-task improvement.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('before', 'after', 'before-environment', 'after-environment', 'report'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    result = compare(*(json.loads(path.read_text()) for path in
        (args.before, args.after, args.before_environment, args.after_environment)))
    save(args.report, result)
    print('Wrote descriptive hardware comparison:', args.report)


if __name__ == '__main__':
    main()
