#!/usr/bin/env python3
"""Adversarial cost-evidence validation without executing retrieval."""
import copy
import unittest
from graph_measurement_evidence import validate_measurements
from evaluate_retrieval import canonical


class MeasurementChecks(unittest.TestCase):
    def row(self):
        payload = {"text": "Memória pública"}
        size = len(canonical(payload))
        return {"method": "semantic", "hits": [{"record": {"payload": payload}}],
                "samples": [{"repetition": 0, "retrieval_ms": 1.0, "http_ms": 2.0,
                             "external_query_embedding_ms": 3.0, "embedding_plus_http_ms": 5.0,
                             "response_bytes": size + 100, "payload_bytes": size}]}

    def test_valid_unicode_payload_and_zero_durations(self):
        row = self.row(); validate_measurements(row, 1)
        for k in ("retrieval_ms", "http_ms", "external_query_embedding_ms", "embedding_plus_http_ms"):
            row["samples"][0][k] = 0
        validate_measurements(row, 1)

    def test_invalid_numbers_and_repetitions(self):
        for field in ("retrieval_ms", "http_ms", "external_query_embedding_ms", "embedding_plus_http_ms"):
            for bad in (-1, float('nan'), float('inf'), True, '1', None, 10**1000):
                row = self.row(); row['samples'][0][field] = bad
                with self.subTest(field=field, kind=type(bad).__name__), self.assertRaises(ValueError):
                    validate_measurements(row, 1)
        for bad in (True, 0.0, -1, 1):
            row = self.row(); row['samples'][0]['repetition'] = bad
            with self.assertRaises(ValueError): validate_measurements(row, 1)
        row = self.row(); row['samples'] *= 2
        with self.assertRaises(ValueError): validate_measurements(row, 1)
        with self.assertRaises(ValueError): validate_measurements(self.row(), 2)

    def test_inconsistent_totals_and_payloads(self):
        for field, bad in [('retrieval_ms', 3.1), ('embedding_plus_http_ms', 6),
                           ('response_bytes', 1), ('response_bytes', 100.0),
                           ('payload_bytes', 0), ('payload_bytes', True)]:
            row = self.row(); row['samples'][0][field] = bad
            with self.subTest(field=field), self.assertRaises(ValueError): validate_measurements(row, 1)
        row = self.row(); row['method'] = 'lexical'
        with self.assertRaises(ValueError): validate_measurements(row, 1)


if __name__ == '__main__':
    unittest.main()
