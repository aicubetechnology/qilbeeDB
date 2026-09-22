"""Regression checks for partial telemetry and invalid counter comparisons."""
import unittest
from unittest.mock import patch
from retrieval_resources import parse_metrics, resource_delta, container_resources


class ResourceEvidenceTests(unittest.TestCase):
    def test_optional_controllers_do_not_erase_cpu(self):
        result, missing = parse_metrics({'cpu.stat': 'usage_usec 9\nnr_throttled 2', 'io.stat': ''})
        self.assertEqual(result['cpu.stat']['usage_usec'], 9)
        self.assertEqual(result['io.stat'], {})
        self.assertIn('memory.peak', missing)

    def test_limits_and_per_device_counters(self):
        result, missing = parse_metrics({'cpu.max': '200000 100000', 'memory.max': 'max',
            'io.stat': '8:0 rbytes=4096 wbytes=8192 rios=1 wios=2\n8:16 rbytes=0 wbytes=1',
            'memory.events': 'oom 0\noom_kill 0'})
        self.assertEqual(result['cpu.max']['quota_usec'], 200000)
        self.assertIsNone(result['memory.max'])
        self.assertEqual(result['io.stat']['8:0']['wbytes'], 8192)
        self.assertNotIn('io.stat', missing)

    def test_malformed_counters_remain_unknown(self):
        for raw in ['usage_usec -1', 'usage_usec 1\nusage_usec 2', 'usage_usec nope']:
            result, missing = parse_metrics({'cpu.stat': raw})
            self.assertNotIn('cpu.stat', result)
            self.assertIn('cpu.stat', missing)
        self.assertIn('cpu.max', parse_metrics({'cpu.max': 'max 0'})[1])

    def test_restart_and_counter_reset_do_not_form_a_delta(self):
        before = {'available': True, 'container_identity': {'id': 'same', 'started_at': 'a'}, 'cpu_usage_usec': 20}
        self.assertEqual(resource_delta(before, dict(before, cpu_usage_usec=30)), 10)
        self.assertIsNone(resource_delta(before, dict(before, cpu_usage_usec=10)))
        self.assertIsNone(resource_delta(before, dict(before, container_identity={'id': 'same', 'started_at': 'b'})))
        self.assertIsNone(resource_delta(before, dict(before, available=False)))

    def test_missing_optional_file_keeps_available_cpu(self):
        import subprocess
        def read(args, **kwargs):
            from types import SimpleNamespace
            if args[-1].endswith("memory.peak"):
                raise subprocess.CalledProcessError(1, args)
            values = {"cpu.stat": "usage_usec 5", "cpu.max": "max 100000",
                      "memory.current": "1024", "memory.max": "2048",
                      "memory.events": "oom 0", "io.stat": ""}
            return SimpleNamespace(stdout=values[args[-1].rsplit("/", 1)[1]])
        with patch('retrieval_resources.identity', return_value={'id': 'same', 'started_at': 'a'}), patch('retrieval_resources.subprocess.run', side_effect=read):
            sample = container_resources('owned-fixture')
        self.assertTrue(sample['available'])
        self.assertEqual(sample['cpu_usage_usec'], 5)
        self.assertIsNone(sample['memory_peak_since_container_start_bytes'])
        self.assertEqual(sample['unavailable_metrics'], ['memory.peak'])

    def test_restart_during_sample_is_unavailable(self):
        with patch('retrieval_resources.identity', side_effect=[{'id':'a'}, {'id':'b'}]), patch('retrieval_resources.subprocess.run') as run:
            run.return_value.stdout = 'usage_usec 5'
            sample = container_resources('owned-fixture')
        self.assertFalse(sample['available'])
        self.assertFalse(sample['stable_during_sampling'])


if __name__ == '__main__':
    unittest.main()
