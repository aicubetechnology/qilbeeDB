"""Frozen-source and proof validation shared by graph retrieval experiments."""

import copy
import math

from evaluate_retrieval import (
    PROFILES,
    digest,
    save,
    source_payload,
    validate_fixture,
)
from import_musique_graph import document_graph


def graph_profile(version):
    weights = {
        "typed_path_balanced_v1": [1, 1, 1, 1, 1, 1],
        "typed_path_entity_v1": [0.5, 1, 0, 0, 0, 0],
    }[version]
    return {
        "version": version,
        "method": "strongest_typed_path_rrf",
        "experimental": True,
        "traversal_version": "typed_relations_v1",
        "base_candidate_limit": 100,
        "anchor_limit": 4,
        "rank_constant": 2,
        "base_weight": 0.25,
        "graph_weight": 0.75,
        "hop_decay": 0.5,
        "cosine_affinity_floor": 0.5,
        "cosine_affinity_weight": 0.5,
        "missing_embedding_affinity": 0.5,
        "maximum_score": 1 / 3,
        "relation_weights": [
            {"kind": k, "weight": w}
            for k, w in zip(
                [
                    "semantic_related",
                    "same_entity",
                    "temporal_before",
                    "causal_claim",
                    "supports",
                    "contradicts",
                ],
                weights,
            )
        ],
    }


def verify_fixture_graph(fixture, graph):
    validate_fixture(fixture)
    source = copy.deepcopy(fixture)
    source.pop("space")
    source.pop("embedding_provenance")
    for group in ["documents", "queries"]:
        for row in source[group]:
            row.pop("vector")
    if graph["source_sha256"] != digest(source):
        raise ValueError("Graph is bound to another frozen source")
    expected = {d["id"]: d["text"] for d in fixture["documents"]}
    projected = {
        d["id"]: d["title"] + "\n" + d["paragraph_text"] for d in graph["projection"]
    }
    if expected != projected:
        raise ValueError("Graph document projection differs from retrieval source")
    edges, metadata = document_graph(graph["projection"])
    if (
        edges != graph["relations"]
        or any(graph.get(k) != v for k, v in metadata.items() if k != "relations")
        or len(graph["relations"]) != metadata["relations"]
    ):
        raise ValueError("Graph differs from the document-only frozen construction")


def relation_input(edge, state):
    return {
        **{
            name: {
                "record_id": state["documents"][edge[name]]["record_id"],
                "revision": state["documents"][edge[name]]["revision"],
            }
            for name in ["source", "target"]
        },
        "kind": edge["kind"],
        "provenance": edge["provenance"],
        "valid_from_millis": None,
        "valid_until_millis": None,
    }


def prepare_relations(client, fixture, graph, state, path):
    verify_fixture_graph(fixture, graph)
    expected = {"graph_sha256": digest(graph), "fixture_sha256": digest(fixture)}
    if "graph" not in state:
        state["graph"] = dict(expected, relations={})
        save(path, state)
    if any(state["graph"].get(k) != v for k, v in expected.items()):
        raise ValueError("Saved graph belongs to another corpus or extraction")
    relations = state["graph"]["relations"]
    if not set(relations) <= {e["id"] for e in graph["relations"]}:
        raise ValueError("Unknown saved relation")
    for i, edge in enumerate(graph["relations"]):
        if edge["id"] in relations:
            continue
        request = {
            "contract_version": 1,
            "scope": state["scope"],
            "idempotency_key": "graph-benchmark-" + digest([expected, edge["id"]]),
            "operation": {"type": "assert", "relation": relation_input(edge, state)},
        }
        receipt = client.call("POST", "/api/v1/memory/relations/commands", request)[0][
            "receipt"
        ]
        relation = client.call(
            "POST",
            "/api/v1/memory/relations/read",
            {
                "contract_version": 1,
                "scope": state["scope"],
                "relation_id": receipt["relation_id"],
            },
        )[0]["relation"]
        if (
            relation["input"] != request["operation"]["relation"]
            or relation["revision"] != receipt["revision"]
        ):
            raise ValueError("New relation does not match acknowledged input")
        relations[edge["id"]] = relation
        if i % 64 == 0:
            save(path, state)
    save(path, state)


def verify_relations(client, graph, state):
    if set(state["graph"]["relations"]) != {e["id"] for e in graph["relations"]}:
        raise ValueError("Incomplete frozen relation manifest")
    for edge in graph["relations"]:
        expected = state["graph"]["relations"][edge["id"]]
        actual = client.call(
            "POST",
            "/api/v1/memory/relations/read",
            {
                "contract_version": 1,
                "scope": state["scope"],
                "relation_id": expected["relation_id"],
            },
        )[0]["relation"]
        if actual != expected or actual["input"] != relation_input(edge, state):
            raise ValueError("Frozen relation, provenance or endpoint revision changed")


def fences(client, scope):
    result = {}
    for name, route in [
        ("memory", "/api/v2/memory/changes"),
        ("relations", "/api/v1/memory/relations/changes"),
    ]:
        page = client.call(
            "POST",
            route,
            {
                "contract_version": 2 if name == "memory" else 1,
                "scope": scope,
                "query": {"limit": 1},
            },
        )[0]["page"]
        if not page["active"]:
            raise ValueError("Expected an active history for the prepared corpus")
        result[name] = {
            "baseline": page["baseline"],
            "high_watermark": page["high_watermark"],
        }
    return result


def near(actual, expected):
    if (
        type(actual) not in (float, int)
        or not math.isfinite(actual)
        or not math.isclose(actual, expected, rel_tol=2e-6, abs_tol=1e-9)
    ):
        raise ValueError("Score or path strength differs from frozen evidence")


def cosine(a, b):
    return max(
        -1.0,
        min(
            1.0,
            sum(x * y for x, y in zip(a, b))
            / (math.sqrt(sum(x * x for x in a)) * math.sqrt(sum(y * y for y in b))),
        ),
    )


def validate_page(result, fixture, state, query, method, protocol):
    if result.get("scope") != state["scope"] or result.get("contract_version") != 1:
        raise ValueError("Response scope or contract mismatch")
    graph_mode = method.startswith("graph_")
    page = result["page"]
    if graph_mode:
        profile = graph_profile(protocol["graph_profiles"][method])
        if (
            page["ranking"] != profile
            or not page["coverage"]["source_complete"]
            or not page["coverage"]["embeddings_complete"]
        ):
            raise ValueError("Graph profile, source scan or embedding coverage differs")
        expansion = dict(
            protocol["expansion"],
            max_depth=(
                0
                if method.endswith("depth_zero")
                else protocol["expansion"]["max_depth"]
            ),
        )
        if page["expansion"] != expansion or page["seed"]["corpus_records"] != len(
            state["documents"]
        ):
            raise ValueError("Graph expansion or corpus count differs")
        graph_lexical = method == "graph_lexical_balanced"
        if page["seed"]["mode"] != ("lexical" if graph_lexical else "hybrid"):
            raise ValueError("Graph seed method differs")
        if not graph_lexical and (
            page["seed"]["hybrid_profile"] != PROFILES["weighted_rrf_v2"]
            or page["seed"]["embedding_space"] != fixture["space"]
        ):
            raise ValueError("Graph seed profile or model space differs")
        if page["coverage"]["complete"] != all(
            page["coverage"][f]
            for f in [
                "source_complete",
                "candidates_complete",
                "graph_complete",
                "embeddings_complete",
            ]
        ):
            raise ValueError("Contradictory graph coverage")
    else:
        expected_version = {"lexical": "bm25_v1", "semantic": "cosine_exact_v1"}.get(
            method, method
        )
        if (
            result["ranking_version"] != expected_version
            or not page["exhaustive"]
            or page["next_after"] is not None
        ):
            raise ValueError("Baseline profile or source coverage differs")
        count = (
            page["matched_records"] if method == "semantic" else page["corpus_records"]
        )
        if count != len(state["documents"]):
            raise ValueError("Baseline corpus coverage differs")
        if method in PROFILES and (
            page["ranking"] != PROFILES[method]
            or page["embedding_coverage"] != "complete"
        ):
            raise ValueError("Hybrid profile or current embedding coverage differs")
    aliases = {v["record_id"]: k for k, v in state["documents"].items()}
    documents = {d["id"]: d for d in fixture["documents"]}
    ids = [h["record"]["record_id"] for h in page["hits"]]
    if (
        len(ids) > protocol["k"]
        or len(ids) != len(set(ids))
        or not set(ids) <= aliases.keys()
    ):
        raise ValueError("Invalid, duplicate or foreign retrieved record")
    tag = "retrieval-fixture-" + state["fixture_sha256"]
    expected_relations = {
        r["relation_id"]: r
        for r in state.get("graph", {}).get("relations", {}).values()
    }
    proofs = {r["relation_id"]: r for r in page.get("relations", [])}
    if len(proofs) != len(page.get("relations", [])) or any(
        expected_relations.get(k) != v for k, v in proofs.items()
    ):
        raise ValueError("Duplicate or altered relation proof")
    used_proofs = set()
    for hit in page["hits"]:
        record = hit["record"]
        alias = aliases[record["record_id"]]
        binding = state["documents"][alias]
        if record["revision"] != binding["revision"] or record[
            "payload"
        ] != source_payload(documents[alias], tag, state["fixture_sha256"]):
            raise ValueError("Stale or altered source payload")
        embedding = hit.get("embedding") or (hit.get("affinity") or {}).get("embedding")
        if embedding is not None and embedding != binding["embedding"]:
            raise ValueError("Embedding receipt differs from frozen binding")
        if not graph_mode:
            continue
        base, path = hit["base"], hit["graph"]
        if not base and not path:
            raise ValueError("Graph result has no ranking evidence")
        if not 0 <= hit["score"] <= page["ranking"]["maximum_score"]:
            raise ValueError("Graph score exceeds the profile range")
        near(
            hit["score"],
            (base["contribution"] if base else 0)
            + (path["contribution"] if path else 0),
        )
        if base:
            if type(base["rank"]) is not int or not 1 <= base["rank"] <= 100:
                raise ValueError("Base rank exceeds the frozen candidate pool")
            near(base["contribution"], 0.25 / (2 + base["rank"]))
        if path:
            if (
                type(path["rank"]) is not int
                or not 1 <= path["rank"] <= expansion["node_limit"]
            ):
                raise ValueError("Graph rank exceeds the node budget")
            if type(path["anchor_rank"]) is not int or not 1 <= path[
                "anchor_rank"
            ] <= len(page["seed"]["selected_anchors"]):
                raise ValueError("Anchor rank exceeds the selected roots")
            near(path["contribution"], 0.75 / (2 + path["rank"]))
            previous = path["anchor"]
            if (
                previous != page["seed"]["selected_anchors"][path["anchor_rank"] - 1]
                or path["missing_affinity"]
            ):
                raise ValueError("Path anchor or embedding coverage differs")
            strength = 1 / (2 + path["anchor_rank"])
            visited = {previous["record_id"]}
            if len(path["steps"]) > page["expansion"]["max_depth"]:
                raise ValueError("Path exceeds depth budget")
            weights = {
                w["kind"]: w["weight"] for w in page["ranking"]["relation_weights"]
            }
            for step in path["steps"]:
                relation = proofs[step["relation_id"]]
                used_proofs.add(step["relation_id"])
                source, target = (
                    relation["input"]["source"],
                    relation["input"]["target"],
                )
                if step["direction"] == "incoming":
                    source, target = target, source
                elif step["direction"] != "outgoing":
                    raise ValueError("Unknown proof direction")
                if (
                    step["relation_revision"] != relation["revision"]
                    or step["from"] != source
                    or step["to"] != target
                    or previous != source
                    or target["record_id"] in visited
                ):
                    raise ValueError("Broken, cyclic or stale path proof")
                visited.add(target["record_id"])
                affinity = (
                    1
                    if graph_lexical
                    else 0.5
                    + 0.5
                    * max(
                        0,
                        cosine(
                            query["vector"],
                            documents[aliases[target["record_id"]]]["vector"],
                        ),
                    )
                )
                strength *= 0.5 * weights[relation["input"]["kind"]] * affinity
                previous = target
            if previous != {
                "record_id": record["record_id"],
                "revision": record["revision"],
            }:
                raise ValueError("Path does not end at the retrieved revision")
            near(path["strength"], strength)
    if used_proofs != set(proofs):
        raise ValueError("Unrelated assertions were attached to final context")
    return [aliases[rid] for rid in ids]
