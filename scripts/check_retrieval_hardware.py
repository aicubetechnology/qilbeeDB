"""Reject confounded or incomplete hardware comparisons before reporting latency."""
import copy
import unittest
from compare_retrieval_hardware import compare, digest, MODES


def fixture(cpu):
    limit = {'quota_usec': cpu * 100000, 'period_usec': 100000}
    sample = {'available': True, 'stable_during_sampling': True,
              'container_identity': {'id': 'owned', 'started_at': 't'},
              'cgroup_v2': {'cpu.max': limit, 'memory.max': 1024}, 'unavailable_metrics': []}
    report = {'valid_comparison': True, 'failures': [], 'violations': {},
              'trial_plan': {'repetitions': 2},
              'fixture_sha256': 'fixture', 'manifest_sha256': 'manifest', 'scope_sha256': 'scope',
              'source_revisions': [], 'model_space': {}, 'conditions': {}, 'server_health': {},
              'metrics_definition': {}, 'rows': [],
              'resources': {'before': sample, 'after': copy.deepcopy(sample),
                            'per_method_separate_warm_pass': {mode: {'queries': 1, 'before': copy.deepcopy(sample), 'after': copy.deepcopy(sample)} for mode in MODES}}}
    report['trial_plan_sha256'] = digest(report['trial_plan'])
    for mode in MODES:
        report['rows'].append({'query_id': 'q', 'mode': mode, 'ranked': ['a'],
                              'samples': [{'retrieval_ms': 1, 'http_ms': 2, 'response_bytes': 20, 'response_payload_sha256': 'b'*64} for _ in range(2)]})
    env = {'schema_version': 1, 'report_sha256': digest(report), 'image_digest': 'sha256:' + 'a'*64,
           'environment_id': 'isolated-fixture', 'storage': 'same-volume',
           'background_workload': 'controlled', 'cpu': limit, 'memory': 1024}
    return report, env


class HardwareComparisonTests(unittest.TestCase):
    def setUp(self):
        self.a, self.ae = fixture(1)
        self.b, self.be = fixture(2)

    def run_comparison(self):
        return compare(self.a, self.b, self.ae, self.be)

    def rebind(self):
        self.be['report_sha256'] = digest(self.b)

    def test_valid_one_axis(self):
        result = self.run_comparison()
        self.assertEqual(result['changed_resource'], 'cpu')
        self.assertEqual(len(result['per_query']), 3)

    def test_edited_report_without_new_declaration(self):
        self.b['rows'][0]['samples'][0]['http_ms'] = 3
        with self.assertRaisesRegex(ValueError, 'bind'): self.run_comparison()

    def test_changed_ranking_and_partial_coverage(self):
        self.b['rows'][0]['ranked'] = ['b']; self.rebind()
        with self.assertRaisesRegex(ValueError, 'Ranking'): self.run_comparison()
        self.b['valid_comparison'] = False; self.rebind()
        with self.assertRaisesRegex(ValueError, 'incomplete'): self.run_comparison()

    def test_multiple_axes(self):
        self.be['storage'] = 'different-volume'
        with self.assertRaisesRegex(ValueError, 'one resource'): self.run_comparison()

    def test_restart_and_mismatched_limits(self):
        self.b['resources']['after']['container_identity']['started_at'] = 'later'; self.rebind()
        with self.assertRaisesRegex(ValueError, 'restarted'): self.run_comparison()
        self.b, self.be = fixture(2); self.be['memory'] = 999
        with self.assertRaisesRegex(ValueError, 'limits'): self.run_comparison()

    def test_missing_samples_or_nonfinite_latency(self):
        self.b['rows'][0]['samples'].pop(); self.rebind()
        with self.assertRaisesRegex(ValueError, 'Missing latency'): self.run_comparison()
        self.b, self.be = fixture(2)
        self.b['rows'][0]['samples'][0]['http_ms'] = float('inf')
        with self.assertRaisesRegex(ValueError, 'JSON compliant'): self.run_comparison()

    def test_raw_timing_length_can_vary_but_content_cannot(self):
        self.b['rows'][0]['samples'][0]['response_bytes'] += 1; self.rebind()
        self.run_comparison()
        self.b['rows'][0]['samples'][0]['response_payload_sha256'] = 'c'*64; self.rebind()
        with self.assertRaisesRegex(ValueError, 'content differs'): self.run_comparison()

    def test_declared_coverage_cannot_exceed_retained_rows(self):
        self.b['resources']['per_method_separate_warm_pass']['lexical']['queries'] = 2
        self.rebind()
        with self.assertRaisesRegex(ValueError, 'recorded resource-pass coverage'): self.run_comparison()

    def test_query_duplicates(self):
        self.b['rows'].append(copy.deepcopy(self.b['rows'][0])); self.rebind()
        with self.assertRaisesRegex(ValueError, 'Duplicate'): self.run_comparison()


if __name__ == '__main__': unittest.main()
