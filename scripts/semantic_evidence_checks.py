#!/usr/bin/env python3
"""Offline adversarial checks; synthetic evidence does not measure relevance."""
import copy
import unittest
from unittest.mock import patch
from semantic_evidence import frozen_cosine, validate_semantic_evidence
from graph_retrieval_checks import proof_fixture
from graph_evaluation_contract import validate_page
from evaluate_retrieval import evaluate_locked, trial_plan, source_payload, PROFILES

class SemanticEvidenceChecks(unittest.TestCase):
    def setUp(self):
        self.binding = {'embedding': {'vector_digest': 'frozen', 'model': 'external-v1'}}
        self.hit = {'embedding': self.binding['embedding'], 'score': 1.0}

    def test_valid_cosines_and_float32_rounding(self):
        for a,b,score in [([1,0],[1,0],1),([1,0],[-1,0],-1),([1,0],[0,1],0)]:
            validate_semantic_evidence(dict(self.hit,score=score),'semantic',self.binding,a,b)
        self.assertEqual(frozen_cosine([16777217,1],[16777216,1]),1.0)
        for dimensions in (1536,3072,32768):
            self.assertAlmostEqual(frozen_cosine([1]*dimensions,[1]*dimensions),1.0)

    def test_missing_null_and_changed_receipts(self):
        for receipt in (None,{}, {'vector_digest':'different'}):
            with self.assertRaises(ValueError):
                validate_semantic_evidence(dict(self.hit,embedding=receipt),'semantic',self.binding,[1,0],[1,0])
        missing=dict(self.hit);missing.pop('embedding')
        with self.assertRaises(ValueError):validate_semantic_evidence(missing,'semantic',self.binding,[1,0],[1,0])

    def test_in_range_fabricated_and_non_numeric_scores(self):
        for score in (.125,True,None,'1',float('nan'),float('inf'),10**1000,1.01):
            with self.assertRaises(ValueError):
                validate_semantic_evidence(dict(self.hit,score=score),'semantic',self.binding,[1,0],[1,0])

    def test_hybrid_channel_receipt_and_cosine(self):
        for method in ('hybrid','weighted_rrf_v1','weighted_rrf_v2'):
            hit={'embedding':self.binding['embedding'],'semantic':{'score':1.0},'score':.25}
            validate_semantic_evidence(hit,method,self.binding,[1,0],[1,0])
            for bad in ({**hit,'embedding':None},{**hit,'semantic':None},{**hit,'semantic':{'score':.25}}):
                with self.assertRaises(ValueError):validate_semantic_evidence(bad,method,self.binding,[1,0],[1,0])
            validate_semantic_evidence({'semantic':None,'embedding':None},method,self.binding,[1,0],[1,0])

    def test_invalid_vectors_fail_without_truncation(self):
        for a,b in [([1],[1,0]),([] ,[]),([0,0],[1,0]),([True,0],[1,0]),([float('nan')],[1]),([1e100],[1])]:
            with self.assertRaises(ValueError):frozen_cosine(a,b)

    def test_graph_baseline_rejects_previously_accepted_evidence(self):
        v,_,s,p,_=proof_fixture();d=v['documents'][0];b=s['documents'][d['id']]
        hit={'record':{'record_id':b['record_id'],'revision':b['revision'],'payload':source_payload(d,'retrieval-fixture-'+s['fixture_sha256'],s['fixture_sha256'])},'score':.125}
        result={'contract_version':1,'scope':s['scope'],'ranking_version':'cosine_exact_v1','page':{'hits':[hit],'exhaustive':True,'next_after':None,'matched_records':2}}
        with self.assertRaises(ValueError):validate_page(result,v,s,v['queries'][1],'semantic',p)
        hit['embedding']=b['embedding']
        with self.assertRaises(ValueError):validate_page(result,v,s,v['queries'][1],'semantic',p)
        hit['score']=1.0
        self.assertEqual(len(validate_page(result,v,s,v['queries'][1],'semantic',p)),1)

    def test_full_three_mode_report_rejects_missing_or_fabricated_evidence(self):
        fixture,_,state,_,_=proof_fixture()
        for binding in state['documents'].values():binding['embedding']['vector_digest']='fixture-digest'
        tag='retrieval-fixture-'+state['fixture_sha256'];plan=trial_plan(fixture,repetitions=1)
        class Client:
            damage=None
            def call(inner,method,path,body=None):
                if path=='/health':return ({'version':'synthetic'},1,1)
                mode=body.get('mode','semantic');hits=[]
                for document in fixture['documents']:
                    binding=state['documents'][document['id']]
                    hit={'record':{'record_id':binding['record_id'],'revision':binding['revision']},'score':1.0}
                    if mode!='lexical':hit['embedding']=binding['embedding']
                    if mode=='hybrid':hit['semantic']={'score':1.0};hit['score']=1/61
                    if mode=='semantic' and inner.damage=='missing':hit.pop('embedding')
                    if mode=='semantic' and inner.damage=='fabricated':hit['score']=.125
                    hits.append(hit)
                page={'hits':hits,'exhaustive':True,'next_after':None,'matched_records':2,'corpus_records':2,'embedding_coverage':'complete','ranking':PROFILES['weighted_rrf_v1']}
                return ({'scope':state['scope'],'mode':mode,'ranking_version':{'lexical':'bm25_v1','semantic':'cosine_exact_v1','hybrid':'weighted_rrf_v1'}[mode],'page':page,'timing':{'retrieval_micros':1}},1,1)
        with patch('evaluate_retrieval.prepare',return_value=(state,tag)),patch('evaluate_retrieval.verify_sources'),patch('evaluate_retrieval.container_resources',return_value=None):
            client=Client();report=evaluate_locked(client,fixture,state['scope'],None,plan)
            self.assertTrue(report['valid_comparison'],report['failures'])
            for damage in ('missing','fabricated'):
                client.damage=damage;report=evaluate_locked(client,fixture,state['scope'],None,plan)
                self.assertFalse(report['valid_comparison']);self.assertTrue(report['failures'])
                self.assertFalse(any(row['mode']=='semantic' for row in report['rows']))

if __name__=='__main__':unittest.main()
