"""Keep missing generation timing distinct from measured zero and partial aggregates."""
import copy
import unittest
from evaluate_graph_retrieval import summary, validate_generation_timings
from evaluate_retrieval import digest
import graph_measurement_checks
from graph_measurement_evidence import validate_measurements
from twowiki_evaluation_checks import synthetic_contract


class BatchTimingChecks(unittest.TestCase):
    def row(self):
        row = graph_measurement_checks.MeasurementChecks().row()
        row['samples'][0].update(external_query_embedding_ms=None, embedding_plus_http_ms=None,
                                embedding_timing_status='unavailable_batch_capture')
        return row

    def test_explicit_paired_nulls_only(self):
        validate_measurements(self.row(), 1)
        for field, value in [('external_query_embedding_ms',0),('embedding_plus_http_ms',2),
                             ('embedding_timing_status',None),('embedding_timing_status','unknown')]:
            row=self.row();row['samples'][0][field]=value
            with self.assertRaises(ValueError):validate_measurements(row,1)
        row=self.row();row['method']='lexical'
        with self.assertRaises(ValueError):validate_measurements(row,1)

    def test_mixed_samples_never_silently_drop_missing_values(self):
        row=self.row();row['metrics']={k:1 for k in ('ndcg_at_10','judged_recall_at_10','all_labeled_supports_at_10','no_useful_result')}
        row['coverage']={'candidates_complete':True}
        numeric=copy.deepcopy(row);numeric['samples']=graph_measurement_checks.MeasurementChecks().row()['samples']
        result=summary([row,numeric])
        self.assertEqual(result['embedding_plus_http_ms'],{'p50':None,'p95':None})
        self.assertEqual(result['http_ms'],{'p50':2.0,'p95':2.0})

    def test_generation_evidence_is_pinned_without_query_allocation(self):
        fixture,_,_,protocol=synthetic_contract()
        generation={'fixture_sha256':digest(fixture),'queries':{},
                    'query_timing_status':'unavailable_batch_capture','batch_evidence':{'sha256':'receipt'}}
        protocol['generation_sha256']=digest(generation)
        validate_generation_timings(generation,fixture,protocol)
        changed=copy.deepcopy(generation);changed['batch_evidence']['sha256']='changed'
        with self.assertRaisesRegex(ValueError,'frozen protocol'):
            validate_generation_timings(changed,fixture,protocol)
        for queries in ({'query':{'elapsed_ms':0}},None):
            changed=dict(generation,queries=queries)
            with self.assertRaises(ValueError):
                validate_generation_timings(changed,fixture,dict(protocol,generation_sha256=digest(changed)))


if __name__=='__main__':unittest.main()
