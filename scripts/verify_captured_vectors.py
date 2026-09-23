"""Verify an offline captured-vector handoff without contacting an embedding provider."""
import argparse
import copy
import hashlib
import json
import math
import struct
from datetime import datetime
from pathlib import Path


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False,
                      allow_nan=False).encode('utf-8')


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    require(path.is_file() and not path.is_symlink(), 'Missing or symlinked evidence file')
    return json.loads(path.read_text())


def verify(directory, source, expected_fixture, expected_manifest):
    """Verify pinned artifacts, exact source mapping, response vectors and batch receipts."""
    fixture = read(directory / 'evaluation-fixture.json')
    manifest = read(directory / 'vector-input-manifest.json')
    require(digest(fixture) == expected_fixture, 'Evaluation fixture digest differs')
    require(digest(manifest) == expected_manifest, 'Input/vector manifest digest differs')
    plan = read(directory / 'plan.json')
    original = read(directory / 'fixture.json')
    measurements = read(directory / 'measurements.json')
    audit = read(directory / 'handoff-audit.json')
    plan_hash, source_hash = digest(plan), digest(source)
    space = fixture['space']
    dimensions = space['dimensions']
    require(type(dimensions) is int and 0 < dimensions <= 32768, 'Invalid dimensions')
    require(space == {'provider': manifest['provider'], 'model': manifest['model'],
                     'dimensions': manifest['dimensions'],
                     'revision': 'captured-' + expected_manifest[:24]}, 'Captured identity differs')
    require(plan['model'] == space['model'] and plan['dimensions'] == dimensions,
            'Generation identity differs')
    require(manifest['source_sha256'] == source_hash == plan['source_sha256'],
            'Source identity differs')
    require(manifest['generation_plan_sha256'] == plan_hash, 'Plan identity differs')
    provenance = fixture['embedding_provenance']
    require(provenance['kind'] == manifest['kind'] == 'captured-vector-set'
            and provenance['manifest_sha256'] == expected_manifest
            and provenance['provider_weight_revision'] is None
            and manifest['provider_weight_revision'] is None
            and provenance['offline_only'] is True
            and provenance['newly_generated_queries_compatible'] is False
            and provenance['provider_drift_excluded'] is False
            and provenance['float32_database_roundtrip_verified'] is False
            and manifest['float32_database_roundtrip_verified'] is False,
            'Snapshot limitations differ')
    raw_hash = digest(original)
    require(provenance['raw_capture_fixture_sha256'] == raw_hash, 'Original fixture differs')
    for candidate in (fixture, original):
        stripped = copy.deepcopy(candidate)
        stripped.pop('space')
        stripped.pop('embedding_provenance')
        for group in ('documents', 'queries'):
            require(len({row['id'] for row in stripped[group]}) == len(stripped[group]),
                    'Duplicate source identifier')
            for row in stripped[group]:
                row.pop('vector')
        require(canonical(stripped) == canonical(source), 'Source fields or order differ')
    require(original['documents'] == fixture['documents']
            and original['queries'] == fixture['queries'], 'Evaluation vectors differ from capture')
    entries, batch_receipts, measured_batches = [], [], []
    offsets = {'documents': 0, 'queries': 0}
    total_tokens = 0
    last_group = 'documents'
    for number, batch in enumerate(plan['batches']):
        group, offset, count = batch['group'], batch['offset'], batch['count']
        require(group in offsets and not (last_group == 'queries' and group == 'documents'),
                'Unexpected batch order')
        last_group = group
        require(type(count) is int and 0 < count <= plan['batch_size']
                and type(offset) is int and offset == offsets[group], 'Batch gap or overlap')
        rows = fixture[group][offset:offset + count]
        require(len(rows) == count and [r['id'] for r in rows] == batch['input_ids'],
                'Batch identifiers differ')
        request_hash = digest({'model': space['model'], 'dimensions': dimensions,
                               'encoding_format': 'float', 'input': [r['text'] for r in rows]})
        capture = read(directory / f'{number:04d}.capture.json')
        dispatched = read(directory / f'{number:04d}.dispatched.json')
        require(batch['request_sha256'] == request_hash == capture['request_sha256']
                == dispatched['request_sha256'], 'Request content digest differs')
        require(capture['plan_sha256'] == dispatched['plan_sha256'] == plan_hash,
                'Receipt plan differs')
        require(capture['http_status'] == 200, 'Unsuccessful captured response')
        require(datetime.fromisoformat(dispatched['dispatched_at']) <=
                datetime.fromisoformat(capture['captured_at']), 'Receipt chronology differs')
        body = capture['body']
        require(body['model'] == space['model'], 'Returned model differs')
        data = body['data']
        require(len(data) == count and all(type(r['index']) is int for r in data)
                and sorted(r['index'] for r in data) == list(range(count)),
                'Missing or duplicate response index')
        vectors = {r['index']: r['embedding'] for r in data}
        for index, row in enumerate(rows):
            vector = row['vector']
            require(vector == vectors[index], 'Vector differs from captured response')
            require(len(vector) == dimensions and all(type(v) in (int, float)
                    and math.isfinite(v) for v in vector) and any(v != 0 for v in vector),
                    'Invalid vector components')
            packed = struct.pack('<' + str(dimensions) + 'f', *vector)
            converted = struct.unpack('<' + str(dimensions) + 'f', packed)
            require(all(math.isfinite(v) for v in converted) and any(v != 0 for v in converted),
                    'Invalid float32 conversion')
            entries.append({'batch': number, 'batch_index': index, 'dimensions': dimensions,
                            'float32_le_sha256': hashlib.sha256(packed).hexdigest(),
                            'input_utf8_sha256': hashlib.sha256(row['text'].encode()).hexdigest(),
                            'ordinal': offset + index, 'provider_vector_json_sha256': digest(vector),
                            'role': group, 'source_id': row['id']})
        usage = body['usage']['total_tokens']
        require(type(usage) is int and usage >= 0, 'Unknown or invalid token usage')
        total_tokens += usage
        batch_receipts.append({'batch': number, 'captured_at': capture['captured_at'],
            'dispatched_at': dispatched['dispatched_at'], 'http_status': 200,
            'monetary_cost_status': 'not_calculated', 'monetary_cost_usd': None,
            'request_sha256': request_hash, 'requested_model': space['model'],
            'response_capture_sha256': digest(capture), 'returned_model': body['model'],
            'total_tokens': usage})
        measured_batches.append({'batch': number, 'capture_sha256': digest(capture),
                                 'count': count, 'group': group, 'offset': offset, 'tokens': usage})
        offsets[group] += count
    require(all(offsets[g] == len(fixture[g]) for g in offsets), 'Incomplete capture coverage')
    require(manifest['entries'] == entries, 'Input/vector entry manifest differs')
    require(audit['batches'] == batch_receipts and measurements['batches'] == measured_batches,
            'Batch receipt summary differs')
    require(audit['total_tokens'] == measurements['total_tokens'] == total_tokens
            and audit['known_usage_batches'] == len(batch_receipts)
            and audit['unknown_usage_batches'] == 0, 'Usage summary differs')
    require(audit['evaluation_fixture_sha256'] == expected_fixture
            and audit['manifest_sha256'] == expected_manifest
            and audit['original_capture_fixture_sha256'] == raw_hash
            and audit['space'] == space
            and measurements['fixture_sha256'] == raw_hash
            and measurements['plan_sha256'] == plan_hash, 'Receipt artifact binding differs')
    require(audit['monetary_cost_usd'] is None
            and audit['monetary_cost_status'] == 'not_calculated', 'Unknown cost presented as known')
    return {'status': 'verified_offline', 'fixture_sha256': expected_fixture,
            'manifest_sha256': expected_manifest, 'source_sha256': source_hash,
            'plan_sha256': plan_hash, 'space': space, 'counts': offsets,
            'verified_batches': len(batch_receipts), 'total_tokens': total_tokens,
            'monetary_cost_usd': None, 'float32_database_roundtrip_verified': False,
            'provider_weight_revision_verified': False, 'retrieval_executed': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', required=True, type=Path)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--fixture-sha256', required=True)
    parser.add_argument('--manifest-sha256', required=True)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = verify(args.directory, read(args.source), args.fixture_sha256, args.manifest_sha256)
    with args.output.open('x') as stream:
        json.dump(result, stream, indent=2, allow_nan=False)
        stream.write('\n')


if __name__ == '__main__':
    main()
