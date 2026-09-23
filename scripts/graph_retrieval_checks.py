#!/usr/bin/env python3
"""Adversarial checks for the frozen graph evaluation pipeline (no live provider)."""

import copy
import json
from pathlib import Path
import tempfile
import unittest

from evaluate_retrieval import PROFILES, digest, metrics, source_payload
from evaluate_graph_retrieval import request_for, summary, verify_protocol, evaluation_stage, evaluation_queries, verify_seed_baselines
from export_graph_evaluation import export
from graph_report_evidence import categories, comparisons, seed_comparisons
from graph_evaluation_contract import (
    fences,
    graph_profile,
    near,
    prepare_relations,
    relation_input,
    validate_page,
    validate_ranking,
    verify_fixture_graph,
    verify_relations,
)
from import_musique_graph import document_graph, identity


def fixture():
    projection = [
        {"title": "Alpha City", "paragraph_text": "Beta Person founded the library."},
        {
            "title": "Beta Person",
            "paragraph_text": "The researcher studied navigation.",
        },
    ]
    for d in projection:
        d["id"] = identity(d["title"], d["paragraph_text"])
    source = {
        "schema_version": 1,
        "kind": "synthetic_contract",
        "provenance": "Fictional pipeline checks, not relevance evidence",
        "documents": [
            {"id": d["id"], "text": d["title"] + "\n" + d["paragraph_text"]}
            for d in projection
        ],
        "queries": [
            {
                "id": split,
                "text": split + " query",
                "split": split,
                "category": "pipeline",
                "judgments": {projection[0]["id"]: 1},
                "judgments_complete": False,
            }
            for split in ["development", "test"]
        ],
    }
    edges, metadata = document_graph(projection)
    graph = dict(
        metadata,
        schema_version=1,
        source_sha256=digest(source),
        projection=projection,
        relations=edges,
    )
    value = copy.deepcopy(source)
    value.update(
        space={
            "provider": "fixture",
            "model": "fake",
            "revision": "v1",
            "dimensions": 2,
        },
        embedding_provenance="Handwritten contract vectors",
    )
    for group in ["documents", "queries"]:
        for d in value[group]:
            d["vector"] = [1.0, 0.0]
    return value, graph


def proof_fixture():
    value, graph = fixture()
    protocol = json.loads(
        (
            Path(__file__).resolve().parents[1]
            / "benchmarks/retrieval/graph-multihop-protocol-v1.json"
        ).read_text()
    )
    state = {
        "fixture_sha256": digest(value),
        "scope": {
            "project_id": "project",
            "agent_id": "agent",
            "mission_id": None,
            "visibility": "private",
        },
        "documents": {
            d["id"]: {
                "record_id": f"00000000-0000-4000-8000-{i+1:012d}",
                "revision": 1,
                "embedding": {"receipt_id": str(i)},
            }
            for i, d in enumerate(value["documents"])
        },
    }
    edge = graph["relations"][0]
    relation = {
        "relation_id": "00000000-0000-4000-8000-000000000099",
        "revision": 1,
        "input": relation_input(edge, state),
    }
    state["graph"] = {"relations": {edge["id"]: relation}}
    source, target = relation["input"]["source"], relation["input"]["target"]
    document = next(d for d in value["documents"] if d["id"] == edge["target"])
    page = {
        "ranking": graph_profile("typed_path_balanced_v1"),
        "coverage": {
            k: True
            for k in [
                "source_complete",
                "candidates_complete",
                "graph_complete",
                "embeddings_complete",
                "complete",
            ]
        },
        "expansion": protocol["expansion"],
        "seed": {"corpus_records": 2, "mode": "lexical", "selected_anchors": [source]},
        "relations": [relation],
        "hits": [
            {
                "record": dict(
                    target,
                    payload=source_payload(
                        document,
                        "retrieval-fixture-" + state["fixture_sha256"],
                        state["fixture_sha256"],
                    ),
                ),
                "base": None,
                "score": 0.25,
                "graph": {
                    "contribution": 0.25,
                    "rank": 1,
                    "anchor": source,
                    "anchor_rank": 1,
                    "missing_affinity": [],
                    "strength": 1 / 6,
                    "steps": [
                        {
                            "relation_id": relation["relation_id"],
                            "relation_revision": 1,
                            "direction": "outgoing",
                            "from": source,
                            "to": target,
                        }
                    ],
                },
            }
        ],
    }
    result = {"contract_version": 1, "scope": state["scope"], "page": page}
    return value, graph, state, protocol, result


class GraphPipelineChecks(unittest.TestCase):
    def test_roundtrip_and_label_independent_construction(self):
        value, graph = fixture()
        verify_fixture_graph(value, graph)
        changed = copy.deepcopy(value)
        changed["queries"][1]["judgments"] = {value["documents"][1]["id"]: 1}
        changed["queries"][1]["text"] = "An unrelated reserved query"
        with self.assertRaises(ValueError):
            verify_fixture_graph(changed, graph)
        # Refreezing source judgments changes the source identity, but cannot change document edges.
        source = copy.deepcopy(changed)
        source.pop("space")
        source.pop("embedding_provenance")
        for group in ["documents", "queries"]:
            for d in source[group]:
                d.pop("vector")
        same_graph = dict(graph, source_sha256=digest(source))
        verify_fixture_graph(changed, same_graph)
        self.assertEqual(
            document_graph(graph["projection"])[0], same_graph["relations"]
        )

    def test_graph_rejects_label_fields_and_unbound_document_identity(self):
        _, graph = fixture()
        for field in [
            "question",
            "answer",
            "is_supporting",
            "judgments",
            "question_decomposition",
        ]:
            docs = copy.deepcopy(graph["projection"])
            docs[0][field] = "injected"
            with self.assertRaises(ValueError):
                document_graph(docs)
        docs = copy.deepcopy(graph["projection"])
        docs[0]["paragraph_text"] = "altered"
        with self.assertRaises(ValueError):
            document_graph(docs)

    def test_graph_rejects_forged_edges_and_text_drift(self):
        value, graph = fixture()
        changed = copy.deepcopy(graph)
        changed["relations"][0]["kind"] = "supports"
        changed["relations_sha256"] = digest(changed["relations"])
        with self.assertRaises(ValueError):
            verify_fixture_graph(value, changed)
        changed = copy.deepcopy(value)
        changed["documents"][0]["text"] += " changed"
        with self.assertRaises(ValueError):
            verify_fixture_graph(changed, graph)

    def test_whole_title_matching_is_deterministic_and_has_no_self_edges(self):
        _, graph = fixture()
        documents = graph["projection"]
        baseline, _ = document_graph(documents)
        self.assertEqual(baseline, document_graph(list(reversed(documents)))[0])
        self.assertEqual(len(baseline), 1)
        self.assertNotEqual(baseline[0]["source"], baseline[0]["target"])
        docs = copy.deepcopy(documents)
        docs[0]["paragraph_text"] = "Beta Persons visited."
        docs[0]["id"] = identity(docs[0]["title"], docs[0]["paragraph_text"])
        self.assertEqual(document_graph(docs)[0], [])

    def test_lost_read_after_relation_commit_reuses_exact_command(self):
        value, graph = fixture()
        state = {
            "fixture_sha256": digest(value),
            "scope": {
                "project_id": "project",
                "agent_id": "agent",
                "mission_id": None,
                "visibility": "private",
            },
            "documents": {
                d["id"]: {
                    "record_id": f"00000000-0000-4000-8000-{i+1:012d}",
                    "revision": 1,
                }
                for i, d in enumerate(value["documents"])
            },
        }

        class Transport:
            def __init__(self):
                self.requests = []
                self.stored = {}
                self.fail = True

            def call(self, method, path, body):
                if path.endswith("/commands"):
                    self.requests.append(copy.deepcopy(body))
                    key = body["idempotency_key"]
                    self.stored.setdefault(
                        key,
                        {
                            "relation_id": "00000000-0000-4000-8000-000000000099",
                            "revision": 1,
                            "input": body["operation"]["relation"],
                        },
                    )
                    return (
                        {
                            "receipt": {
                                "relation_id": self.stored[key]["relation_id"],
                                "revision": 1,
                            }
                        },
                        0,
                        0,
                    )
                if self.fail:
                    self.fail = False
                    raise RuntimeError("lost read after remote assertion commit")
                return {"relation": next(iter(self.stored.values()))}, 0, 0

        client = Transport()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.json"
            with self.assertRaises(RuntimeError):
                prepare_relations(client, value, graph, state, path)
            resumed = json.loads(path.read_text())
            self.assertEqual(resumed["graph"]["relations"], {})
            prepare_relations(client, value, graph, resumed, path)
            self.assertEqual(len(client.stored), 1)
            self.assertEqual(client.requests[0], client.requests[1])
            self.assertEqual(len(resumed["graph"]["relations"]), 1)

    def test_protocol_rejects_changed_graph_and_comparison_budget(self):
        value, graph = fixture()
        protocol = json.loads(
            (
                Path(__file__).resolve().parents[1]
                / "benchmarks/retrieval/graph-multihop-protocol-v1.json"
            ).read_text()
        )
        protocol.update(
            source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"],
            graph_policy_sha256=graph["policy_sha256"],
        )
        verify_protocol(protocol, value, graph)
        for field, bad in [
            ("k", 11),
            ("source_sha256", "bad"),
            ("client_concurrency", 2),
            ("repetitions", 0),
            ("split", "development"),
            ("graph_seed_hybrid_version", "weighted_rrf_v1"),
            (
                "graph_profiles",
                dict(
                    protocol["graph_profiles"],
                    graph_hybrid_entity="typed_path_balanced_v1",
                ),
            ),
            (
                "primary_comparison",
                {"candidate": "graph_hybrid_entity", "baseline": "semantic"},
            ),
            ("default_admission", True),
        ]:
            with self.assertRaises(ValueError):
                verify_protocol(dict(protocol, **{field: bad}), value, graph)
        with self.assertRaises(ValueError):
            verify_protocol(
                dict(protocol, methods=protocol["methods"] + ["lexical"]), value, graph
            )

    def test_development_protocol_cannot_query_or_claim_reserved_split(self):
        value, graph = fixture()
        protocol = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-multihop-development-v1.json").read_text())
        protocol.update(source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"], graph_policy_sha256=graph["policy_sha256"])
        verify_protocol(protocol, value, graph)
        self.assertEqual(evaluation_stage(protocol), "development")
        self.assertEqual(set(evaluation_queries(value, protocol)), {"development"})
        for change in ({"split": "test"}, {"protocol_version": "graph_multihop_compare_v1"},
            {"protocol_version": "unknown"}, {"default_admission": True}):
            with self.assertRaises(ValueError): verify_protocol(dict(protocol, **change), value, graph)
        empty = copy.deepcopy(value)
        empty["queries"] = [q for q in empty["queries"] if q["split"] == "test"]
        with self.assertRaises(ValueError): evaluation_queries(empty, protocol)

    def test_base_preserving_trial_is_separate_and_uses_immutable_profile(self):
        value, graph = fixture()
        protocol = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-base-preserving-development-v1.json").read_text())
        protocol.update(source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"], graph_policy_sha256=graph["policy_sha256"])
        verify_protocol(protocol, value, graph)
        self.assertEqual(len(protocol["methods"]), 9)
        profile = graph_profile("typed_path_base_preserving_v1")
        self.assertEqual((profile["base_weight"], profile["graph_weight"]), (0.75, 0.25))
        old = graph_profile("typed_path_balanced_v1")
        self.assertEqual((old["base_weight"], old["graph_weight"]), (0.25, 0.75))
        for key in set(old) - {"version", "base_weight", "graph_weight"}:
            self.assertEqual(profile[key], old[key])
        for change in ({"split": "test"}, {"protocol_version": "graph_multihop_development_v1"},
            {"primary_comparison": {"candidate": "graph_hybrid_balanced", "baseline": "weighted_rrf_v2"}}):
            with self.assertRaises(ValueError):
                verify_protocol(dict(protocol, **change), value, graph)

    def test_all_requests_use_the_same_frozen_vectors_scope_and_result_limit(self):
        value, graph = fixture()
        protocol = json.loads(
            (
                Path(__file__).resolve().parents[1]
                / "benchmarks/retrieval/graph-multihop-protocol-v1.json"
            ).read_text()
        )
        state = {
            "scope": {
                "project_id": "p",
                "agent_id": "a",
                "mission_id": None,
                "visibility": "private",
            },
            "fixture_sha256": digest(value),
        }
        for method in protocol["methods"]:
            _, body = request_for(method, value["queries"][1], value, state, protocol)
            self.assertEqual(body["scope"], state["scope"])
            self.assertEqual(body["query"]["limit"], 10)
            query = body["query"].get("seed", body["query"])
            if method not in ["lexical", "graph_lexical_balanced"]:
                self.assertEqual(query["vector"], value["queries"][1]["vector"])
                self.assertEqual(query["space"], value["space"])
            self.assertEqual(
                body["query"]["tag"], "retrieval-fixture-" + state["fixture_sha256"]
            )

    def test_baseline_rejects_invalid_scores_and_out_of_order_evidence(self):
        value, _, state, protocol, _ = proof_fixture()
        hits = []
        for document in value["documents"]:
            binding = state["documents"][document["id"]]
            hits.append({
                "record": dict(record_id=binding["record_id"], revision=binding["revision"],
                    payload=source_payload(document, "retrieval-fixture-" + state["fixture_sha256"], state["fixture_sha256"])),
                "embedding": binding["embedding"], "score": 1.0,
            })
        hits.sort(key=lambda hit: hit["record"]["record_id"])
        result = {"contract_version": 1, "scope": state["scope"],
            "ranking_version": "cosine_exact_v1", "page": {"hits": hits,
                "exhaustive": True, "next_after": None, "matched_records": 2}}
        def check(candidate):
            return validate_page(candidate, value, state, value["queries"][1], "semantic", protocol)
        self.assertEqual(len(check(result)), 2)
        for bad in [float("nan"), float("inf"), -float("inf"), True, "1", None, 10**1000, -1.01, 1.01]:
            altered = copy.deepcopy(result)
            altered["page"]["hits"][0]["score"] = bad
            with self.subTest(score=type(bad).__name__), self.assertRaises(ValueError):
                check(altered)
        reversed_tie = copy.deepcopy(result)
        reversed_tie["page"]["hits"].reverse()
        with self.assertRaises(ValueError):
            check(reversed_tie)
        ascending = copy.deepcopy(result)
        ascending["page"]["hits"][0]["score"] = 0.1
        ascending["page"]["hits"][1]["score"] = 0.9
        with self.assertRaises(ValueError):
            check(ascending)
        descending = copy.deepcopy(ascending)
        descending["page"]["hits"].reverse()
        with self.assertRaises(ValueError):
            check(descending)
        validate_ranking(descending["page"]["hits"], "semantic", 1)
        for method, maximum in [("lexical", None), ("weighted_rrf_v1", 1 / 61), ("weighted_rrf_v2", 1 / 3)]:
            candidate = copy.deepcopy(result)
            candidate["ranking_version"] = "bm25_v1" if method == "lexical" else method
            candidate["page"].update(corpus_records=2, embedding_coverage="complete")
            if method in PROFILES:
                candidate["page"]["ranking"] = PROFILES[method]
                for hit in candidate["page"]["hits"]:
                    hit["semantic"] = {"rank": 1, "score": 1.0, "contribution": maximum}
            for hit in candidate["page"]["hits"]:
                hit["score"] = maximum if maximum is not None else 1.0
            def check_method(page):
                return validate_page(page, value, state, value["queries"][1], method, protocol)
            self.assertEqual(len(check_method(candidate)), 2)
            for invalid in [-0.1, float("nan"), True] + ([maximum + 0.001] if maximum is not None else []):
                bad = copy.deepcopy(candidate)
                bad["page"]["hits"][0]["score"] = invalid
                with self.subTest(method=method), self.assertRaises(ValueError):
                    check_method(bad)
        signed_zero = copy.deepcopy(result)
        signed_zero["page"]["hits"][0]["score"] = -0.0
        signed_zero["page"]["hits"][1]["score"] = 0.0
        with self.assertRaises(ValueError):
            check(signed_zero)
        signed_zero["page"]["hits"].reverse()
        validate_ranking(signed_zero["page"]["hits"], "semantic", 1)
        with self.assertRaises(ValueError):
            check(signed_zero)

    def test_score_verifier_rejects_nonfinite_and_wrong_values(self):
        for bad in [float("nan"), float("inf"), None, "0.5", True, 0.7]:
            with self.assertRaises(ValueError):
                near(bad, 0.5)
        near(0.50000000001, 0.5)

    def test_best_channel_protocol_is_development_only_and_pins_all_methods(self):
        value, graph = fixture()
        protocol = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-best-channel-development-v1.json").read_text())
        protocol.update(source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"], graph_policy_sha256=graph["policy_sha256"])
        verify_protocol(protocol, value, graph)
        self.assertEqual(len(protocol["methods"]), 10)
        self.assertTrue(all(q["split"] == "development" for q in evaluation_queries(value, protocol).values()))
        mutations = [lambda p: p.update(split="test"),
            lambda p: p.update(protocol_version="graph_base_preserving_development_v1"),
            lambda p: p["primary_comparison"].update(candidate="graph_hybrid_base_preserving"),
            lambda p: p["graph_profiles"].update(graph_hybrid_best_channel="typed_path_balanced_v1"),
            lambda p: p["methods"].append("graph_hybrid_best_channel"),
            lambda p: p["methods"].remove("weighted_rrf_v1"),
            lambda p: p.update(default_admission=True)]
        for mutate in mutations:
            changed = copy.deepcopy(protocol)
            mutate(changed)
            with self.assertRaises(ValueError):
                verify_protocol(changed, value, graph)

    def test_v1_seed_protocol_cannot_be_relabelled_as_v2_or_reserved(self):
        value, graph = fixture()
        protocol = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-best-channel-v1-seed-development-v1.json").read_text())
        protocol.update(source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"], graph_policy_sha256=graph["policy_sha256"])
        verify_protocol(protocol, value, graph)
        self.assertEqual(protocol["primary_comparison"]["baseline"], "weighted_rrf_v1")
        mutations = [lambda p: p.update(split="test"),
            lambda p: p.update(protocol_version="graph_best_channel_development_v1"),
            lambda p: p.update(graph_seed_hybrid_version="weighted_rrf_v2"),
            lambda p: p["primary_comparison"].update(baseline="weighted_rrf_v2"),
            lambda p: p["graph_profiles"].update(graph_hybrid_best_channel="typed_path_balanced_v1"),
            lambda p: p["methods"].remove("weighted_rrf_v2"),
            lambda p: p["methods"].append("graph_hybrid_best_channel"),
            lambda p: p.update(default_admission=True)]
        for mutate in mutations:
            changed = copy.deepcopy(protocol)
            mutate(changed)
            with self.assertRaises(ValueError):
                verify_protocol(changed, value, graph)

    def test_seed_request_and_response_must_match_frozen_protocol(self):
        value, _, state, protocol, result = proof_fixture()
        method = "graph_hybrid_balanced"
        for version in ("weighted_rrf_v1", "weighted_rrf_v2"):
            protocol["graph_seed_hybrid_version"] = version
            _, body = request_for(method, value["queries"][1], value, state, protocol)
            self.assertEqual(body["query"]["seed"]["ranking_version"], version)
            result["page"]["seed"].update(mode="hybrid", hybrid_profile=PROFILES[version],
                embedding_space=value["space"])
            validate_page(result, value, state, value["queries"][1], method, protocol)
            changed = copy.deepcopy(result)
            other = "weighted_rrf_v2" if version == "weighted_rrf_v1" else "weighted_rrf_v1"
            changed["page"]["seed"]["hybrid_profile"] = PROFILES[other]
            with self.assertRaises(ValueError):
                validate_page(changed, value, state, value["queries"][1], method, protocol)

    def test_seed_baselines_bind_depth_zero_and_anchors_to_each_version(self):
        for version in ("weighted_rrf_v1", "weighted_rrf_v2"):
            other = "weighted_rrf_v2" if version == "weighted_rrf_v1" else "weighted_rrf_v1"
            protocol = {"graph_seed_hybrid_version": version,
                "graph_profiles": {"graph_hybrid_depth_zero": "unused",
                    "graph_hybrid_best_channel": "unused", "graph_lexical_balanced": "unused"}}
            rows = {}
            for method, ids in [(version, ["a", "b"]), (other, ["c", "d"]),
                    ("lexical", ["e", "f"])]:
                rows["q", method] = {"ranked": ids, "hits": [
                    {"record": {"record_id": rid}} for rid in ids]}
            for method in protocol["graph_profiles"]:
                ids = ["e", "f"] if method == "graph_lexical_balanced" else ["a", "b"]
                rows["q", method] = {"ranked": ids, "work": {"seed": {
                    "selected_anchors": [{"record_id": rid} for rid in ids]}}}
            verify_seed_baselines(rows, ["q"], protocol)
            bad = copy.deepcopy(rows)
            bad["q", "graph_hybrid_depth_zero"]["ranked"] = ["c", "d"]
            with self.assertRaises(ValueError):
                verify_seed_baselines(bad, ["q"], protocol)
            for method in protocol["graph_profiles"]:
                bad = copy.deepcopy(rows)
                bad["q", method]["work"]["seed"]["selected_anchors"] = [{"record_id": "c"}]
                with self.assertRaises(ValueError):
                    verify_seed_baselines(bad, ["q"], protocol)

    def test_seed_matrix_pins_arms_and_rejects_cross_version_overrides(self):
        value, graph = fixture()
        protocol = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-seed-matrix-development-v1.json").read_text())
        protocol.update(source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"], graph_policy_sha256=graph["policy_sha256"])
        verify_protocol(protocol, value, graph)
        self.assertEqual(len(protocol["methods"]), 12)
        mutations = [lambda p: p.update(split="test"),
            lambda p: p.update(graph_seed_hybrid_version="weighted_rrf_v1"),
            lambda p: p["graph_seeds"].update(graph_v2_balanced="weighted_rrf_v1"),
            lambda p: p["graph_seeds"].pop("graph_v1_entity"),
            lambda p: p["graph_profiles"].update(graph_v1_entity="typed_path_balanced_v1"),
            lambda p: p["methods"].remove("graph_v2_depth_zero"),
            lambda p: p["methods"].append("graph_v1_entity"),
            lambda p: p["seed_comparisons"].reverse(),
            lambda p: p.update(default_admission=True)]
        for mutate in mutations:
            changed = copy.deepcopy(protocol)
            mutate(changed)
            with self.assertRaises(ValueError):
                verify_protocol(changed, value, graph)
        legacy = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-best-channel-development-v1.json").read_text())
        legacy.update(source_sha256=graph["source_sha256"],
            relations_sha256=graph["relations_sha256"], graph_policy_sha256=graph["policy_sha256"],
            graph_seeds=protocol["graph_seeds"])
        with self.assertRaises(ValueError):
            verify_protocol(legacy, value, graph)

    def test_matrix_requests_and_controls_bind_each_seed_independently(self):
        value, _, state, _, _ = proof_fixture()
        protocol = json.loads((Path(__file__).resolve().parents[1] /
            "benchmarks/retrieval/graph-seed-matrix-development-v1.json").read_text())
        rows = {}
        for version in (1, 2):
            ids = [f"v{version}-a", f"v{version}-b"]
            rows["q", f"weighted_rrf_v{version}"] = {"ranked": ids,
                "hits": [{"record": {"record_id": rid}} for rid in ids]}
        for method, version in protocol["graph_seeds"].items():
            _, body = request_for(method, value["queries"][1], value, state, protocol)
            self.assertEqual(body["query"]["seed"]["ranking_version"], version)
            self.assertEqual(body["query"]["ranking_version"], protocol["graph_profiles"][method])
            self.assertEqual(body["query"]["expansion"]["max_depth"],
                0 if method.endswith("depth_zero") else 2)
            ids = rows["q", version]["ranked"]
            rows["q", method] = {"ranked": ids, "work": {"seed": {
                "selected_anchors": [{"record_id": rid} for rid in ids]}}}
        verify_seed_baselines(rows, ["q"], protocol)
        for method in protocol["graph_profiles"]:
            changed = copy.deepcopy(rows)
            changed["q", method]["work"]["seed"]["selected_anchors"] = []
            with self.assertRaises(ValueError):
                verify_seed_baselines(changed, ["q"], protocol)
        for method in ("graph_v1_depth_zero", "graph_v2_depth_zero"):
            changed = copy.deepcopy(rows)
            changed["q", method]["ranked"] = ["wrong"]
            with self.assertRaises(ValueError):
                verify_seed_baselines(changed, ["q"], protocol)

    def test_paired_seed_effects_use_query_pairs_not_unpaired_means(self):
        protocol = {"seed": 42, "seed_comparisons": [{"candidate": "v1", "baseline": "v2"}]}
        rows = [{"query_id": q, "method": method, "metrics": {
            metric: value for metric in ("ndcg_at_10", "judged_recall_at_10", "all_labeled_supports_at_10")}}
            for q, method, value in [("a", "v1", 1), ("a", "v2", 0), ("b", "v1", 0), ("b", "v2", 1)]]
        result = seed_comparisons(rows, protocol, {"a": {}, "b": {}})
        self.assertEqual(result, seed_comparisons(list(reversed(rows)), protocol, {"b": {}, "a": {}}))
        metric = result[0]["metrics"]["ndcg_at_10"]
        self.assertEqual((metric["mean_delta"], metric["wins"], metric["losses"], metric["ties"]), (0, 1, 1, 0))
        self.assertLess(metric["paired_interval"]["low"], 0)
        self.assertGreater(metric["paired_interval"]["high"], 0)

    def test_best_channel_uses_maximum_and_rejects_additive_or_weighted_scores(self):
        value, _, state, protocol, result = proof_fixture()
        method = "graph_lexical_balanced"
        protocol["graph_profiles"][method] = "typed_path_best_channel_v1"
        result["page"]["ranking"] = graph_profile("typed_path_best_channel_v1")
        hit = result["page"]["hits"][0]
        hit["graph"]["contribution"] = 1 / 3
        hit["base"] = {"rank": 2, "contribution": 1 / 4}
        hit["score"] = 1 / 3
        def check(candidate):
            return validate_page(candidate, value, state, value["queries"][1], method, protocol)
        self.assertEqual(len(check(result)), 1)
        for score in [1 / 3 + 1 / 4, 0.25 / 3 + 0.75 / 4]:
            forged = copy.deepcopy(result)
            forged["page"]["hits"][0]["score"] = score
            with self.assertRaises(ValueError):
                check(forged)
        forged = copy.deepcopy(result)
        forged["page"]["ranking"]["method"] = "strongest_typed_path_rrf"
        with self.assertRaises(ValueError):
            check(forged)

    def test_new_profile_checks_both_contributions_and_rejects_legacy_weights(self):
        value, _, state, protocol, result = proof_fixture()
        method = "graph_lexical_balanced"
        protocol["graph_profiles"][method] = "typed_path_base_preserving_v1"
        result["page"]["ranking"] = graph_profile("typed_path_base_preserving_v1")
        hit = result["page"]["hits"][0]
        hit["graph"]["contribution"] = 0.25 / 3
        hit["base"] = {"rank": 2, "contribution": 0.75 / 4}
        hit["score"] = 0.25 / 3 + 0.75 / 4
        def check(candidate):
            return validate_page(candidate, value, state, value["queries"][1], method, protocol)
        self.assertEqual(len(check(result)), 1)
        for channel, wrong in [("base", 0.25 / 4), ("graph", 0.75 / 3)]:
            forged = copy.deepcopy(result)
            changed = forged["page"]["hits"][0]
            changed[channel]["contribution"] = wrong
            changed["score"] = changed["base"]["contribution"] + changed["graph"]["contribution"]
            with self.subTest(channel=channel), self.assertRaises(ValueError):
                check(forged)

    def test_frozen_proof_rejects_forgery_stale_revisions_and_scope_drift(self):
        value, _, state, protocol, result = proof_fixture()

        def validate(candidate):
            return validate_page(
                candidate,
                value,
                state,
                value["queries"][1],
                "graph_lexical_balanced",
                protocol,
            )

        self.assertEqual(validate(result), [value["documents"][1]["id"]])
        mutations = [
            lambda r: r["scope"].update(agent_id="another-agent"),
            lambda r: r["page"]["hits"][0]["record"].update(revision=2),
            lambda r: r["page"]["hits"][0]["record"]["payload"].update(text="altered"),
            lambda r: r["page"]["hits"][0].update(score=0.2),
            lambda r: r["page"]["hits"][0]["graph"].update(strength=0.2),
            lambda r: r["page"]["hits"][0]["graph"].update(missing_affinity=True),
            lambda r: r["page"]["hits"][0]["graph"].update(anchor_rank=0),
            lambda r: r["page"]["hits"][0]["graph"].update(rank=0),
            lambda r: r["page"]["hits"][0].update(score=0.4),
            lambda r: r["page"]["hits"][0]["graph"]["steps"][0].update(
                relation_revision=2
            ),
            lambda r: r["page"]["hits"][0]["graph"]["steps"][0].update(
                direction="incoming"
            ),
            lambda r: r["page"]["relations"][0]["input"].update(kind="supports"),
            lambda r: r["page"]["relations"].append(
                copy.deepcopy(r["page"]["relations"][0])
            ),
            lambda r: r["page"]["hits"].append(copy.deepcopy(r["page"]["hits"][0])),
            lambda r: r["page"]["coverage"].update(source_complete=False),
            lambda r: r["page"]["coverage"].update(complete=False),
            lambda r: r["page"]["seed"].update(corpus_records=1),
        ]
        for mutation in mutations:
            changed = copy.deepcopy(result)
            mutation(changed)
            with self.subTest(mutation=mutations.index(mutation)):
                with self.assertRaises(ValueError):
                    validate(changed)

    def test_extra_proofs_and_wrong_endpoint_are_not_context_evidence(self):
        value, _, state, protocol, result = proof_fixture()
        changed = copy.deepcopy(result)
        changed["page"]["hits"][0]["graph"]["steps"] = []
        with self.assertRaisesRegex(ValueError, "does not end"):
            validate_page(
                changed,
                value,
                state,
                value["queries"][1],
                "graph_lexical_balanced",
                protocol,
            )
        # A valid direct anchor requires no assertion; attaching one must fail.
        source = result["page"]["seed"]["selected_anchors"][0]
        result["page"]["hits"][0]["record"] = dict(
            source,
            payload=source_payload(
                value["documents"][0],
                "retrieval-fixture-" + state["fixture_sha256"],
                state["fixture_sha256"],
            ),
        )
        result["page"]["hits"][0]["graph"].update(steps=[], strength=1 / 3)
        with self.assertRaisesRegex(ValueError, "Unrelated assertions"):
            validate_page(
                result,
                value,
                state,
                value["queries"][1],
                "graph_lexical_balanced",
                protocol,
            )

    def test_relation_drift_is_detected_and_failed_reads_are_not_retried(self):
        _, graph, state, _, _ = proof_fixture()

        class Transport:
            def __init__(self):
                self.calls = 0

            def call(self, method, path, body):
                self.calls += 1
                changed = copy.deepcopy(
                    next(iter(state["graph"]["relations"].values()))
                )
                changed["revision"] = 2
                return {"relation": changed}, 0, 0

        client = Transport()
        with self.assertRaisesRegex(ValueError, "changed"):
            verify_relations(client, graph, state)
        self.assertEqual(client.calls, 1)

    def test_both_history_fences_are_required_and_changes_are_observable(self):
        class Transport:
            def __init__(self):
                self.requests = []
                self.sequence = 1
                self.active = True

            def call(self, method, route, body):
                self.requests.append((route, body))
                return (
                    {
                        "page": {
                            "active": self.active,
                            "baseline": 0,
                            "high_watermark": {
                                "sequence": self.sequence,
                                "prefix_digest": "frozen",
                            },
                        }
                    },
                    0,
                    0,
                )

        client = Transport()
        before = fences(client, {"project_id": "p"})
        self.assertEqual(set(before), {"memory", "relations"})
        self.assertEqual(
            [body["contract_version"] for _, body in client.requests], [2, 1]
        )
        client.sequence += 1
        self.assertNotEqual(before, fences(client, {"project_id": "p"}))
        client.active = False
        with self.assertRaisesRegex(ValueError, "active history"):
            fences(client, {})

    def test_export_recalculates_evidence_and_rejects_incomplete_or_changed_results(
        self,
    ):
        value, _, _, protocol, _ = proof_fixture()
        protocol["scope_of_evidence"] = "Synthetic exporter qualification only"
        query = value["queries"][1]
        ranked = list(query["judgments"])
        metric = metrics(ranked, query) | {"all_labeled_supports_at_10": True}
        rows = [
            {
                "query_id": query["id"],
                "category": query["category"],
                "method": method,
                "ranked": ranked,
                "metrics": metric,
                "coverage": {"candidates_complete": True, "graph_complete": True},
                "samples": [
                    {
                        "repetition": rep,
                        "external_query_embedding_ms": 0.0,
                        "response_bytes": 1,
                        "payload_bytes": 0,
                        **{
                            k: 1.0
                            for k in [
                                "retrieval_ms",
                                "http_ms",
                                "embedding_plus_http_ms",
                            ]
                        },
                    }
                    for rep in range(protocol["repetitions"])
                ],
                "work": {},
                "hits": [],
                "relations": [],
            }
            for method in protocol["methods"]
        ]
        plan = {
            k: "synthetic"
            for k in [
                "protocol_sha256",
                "graph_sha256",
                "manifest_sha256",
                "generation_sha256",
                "server_health",
                "execution_limits",
            ]
        }
        plan.update(
            fixture_sha256=digest(value), model_space=value["space"], protocol=protocol,
            protocol_sha256=digest(protocol)
        )
        report = {
            "status": "completed",
            "failures": [],
            "verified_fences_unchanged": True,
            "verified_current_sources_and_relations": True,
            "plan": plan,
            "rows": rows,
            "summary": {
                m: summary([r for r in rows if r["method"] == m])
                for m in protocol["methods"]
            },
            "categories": {},
            "comparisons": {},
            "started_at": "synthetic",
            "finished_at": "synthetic",
            "scope_of_evidence": "Synthetic exporter qualification only",
            "environment": {},
            "resources": dict(
                available=False,
                errors=[],
                peak_sampled_rss_bytes=None,
                cpu_seconds_delta=None,
                scope="not sampled",
            ),
        }
        selected = {query["id"]: query}
        report["categories"] = categories(rows, protocol, selected)
        report["comparisons"] = comparisons(rows, protocol, selected)
        public = export(report, value)
        self.assertEqual(len(public["rows"]), len(protocol["methods"]))
        self.assertNotIn("documents", public["source_provenance"])
        self.assertNotIn("hits", public["rows"][0])
        self.assertNotIn('"vector":', json.dumps(public))
        batch = copy.deepcopy(report)
        batch_protocol = batch["plan"]["protocol"]
        batch_protocol.update(query_timing_status="unavailable_batch_capture", generation_sha256="synthetic")
        batch["plan"]["protocol_sha256"] = digest(batch_protocol)
        for row in batch["rows"]:
            if row["method"] not in ("lexical", "graph_lexical_balanced"):
                for sample in row["samples"]:
                    sample.update(external_query_embedding_ms=None, embedding_plus_http_ms=None,
                                  embedding_timing_status="unavailable_batch_capture")
        batch["summary"] = {m: summary([r for r in batch["rows"] if r["method"] == m])
                            for m in batch_protocol["methods"]}
        batch["categories"] = categories(batch["rows"], batch_protocol, selected)
        batch["comparisons"] = comparisons(batch["rows"], batch_protocol, selected)
        batch_public = export(batch, value)
        self.assertIsNone(batch_public["summary"]["semantic"]["embedding_plus_http_ms"]["p95"])
        self.assertEqual(batch_public["summary"]["semantic"]["http_ms"]["p95"], 1.0)
        self.assertEqual(batch_public["summary"]["lexical"]["embedding_plus_http_ms"]["p95"], 1.0)
        invented = copy.deepcopy(batch)
        semantic_row = next(r for r in invented["rows"] if r["method"] == "semantic")
        for sample in semantic_row["samples"]:
            sample.pop("embedding_timing_status")
            sample.update(external_query_embedding_ms=0.0, embedding_plus_http_ms=sample["http_ms"])
        with self.assertRaisesRegex(ValueError, "availability differs"):
            export(invented, value)
        development = copy.deepcopy(report)
        development["plan"]["protocol"].update(
            protocol_version="graph_multihop_development_v1", split="development")
        development["plan"]["protocol_sha256"] = digest(development["plan"]["protocol"])
        development["evaluation_stage"] = "development"
        for row in development["rows"]: row["query_id"] = "development"
        changed_plan = copy.deepcopy(development)
        changed_plan["plan"]["protocol_sha256"] = "wrong"
        with self.assertRaises(ValueError): export(changed_plan, value)
        published = export(development, value)
        self.assertEqual(published["evaluation_stage"], "development")
        self.assertFalse(published["default_admission"])
        for label in (None, "reserved_comparison"):
            altered = copy.deepcopy(development)
            if label is None: altered.pop("evaluation_stage")
            else: altered["evaluation_stage"] = label
            with self.assertRaises(ValueError): export(altered, value)
        development["rows"][0]["query_id"] = "test"
        with self.assertRaises(ValueError): export(development, value)

        mutations = [
            lambda r: r.update(status="failed"),
            lambda r: r.update(scope_of_evidence="Proven agent improvement"),
            lambda r: r.update(default_admission=True),
            lambda r: r["rows"][0].update(category="fabricated"),
            lambda r: r["categories"][query["category"]]["lexical"].update(ndcg_at_10=999),
            lambda r: r["comparisons"]["weighted_rrf_v2"]["metrics"]["ndcg_at_10"].update(mean_delta=999),
            lambda r: r["comparisons"]["weighted_rrf_v2"]["metrics"]["ndcg_at_10"].update(wins=999),
            lambda r: r["comparisons"]["weighted_rrf_v2"].update(primary_predeclared=False),
            lambda r: r["comparisons"]["weighted_rrf_v2"]["metrics"]["ndcg_at_10"].update(paired_interval={"forged": True}),
            lambda r: r.update(verified_fences_unchanged=False),
            lambda r: r["rows"].pop(),
            lambda r: r["rows"].append(copy.deepcopy(r["rows"][0])),
            lambda r: r["rows"][0]["samples"].pop(),
            lambda r: r["rows"][0]["samples"][0].update(repetition=1),
            lambda r: r["rows"][0]["metrics"].update(ndcg_at_10=0.1),
            lambda r: r["summary"]["lexical"].update(ndcg_at_10=0.1),
            lambda r: r["plan"].update(fixture_sha256="another-source"),
        ]
        for index, mutation in enumerate(mutations):
            changed = copy.deepcopy(report)
            mutation(changed)
            with self.subTest(mutation=index), self.assertRaises(ValueError):
                export(changed, value)


if __name__ == "__main__":
    unittest.main()
