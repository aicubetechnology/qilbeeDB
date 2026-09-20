#!/usr/bin/env python3
"""Independently audit frozen retrieval evidence without issuing HTTP or provider calls."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import statistics

MODES = ("lexical", "semantic", "hybrid")
METHODS = dict(lexical="lexical", semantic="semantic", hybrid="weighted_rrf_v1")
PROFILES = dict(lexical="bm25_v1", semantic="cosine_exact_v1", hybrid="weighted_rrf_v1")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def close(left, right):
    return (
        type(left) in (int, float)
        and type(right) in (int, float)
        and math.isclose(left, right, rel_tol=1e-12, abs_tol=1e-12)
    )


def number(value):
    return type(value) in (int, float) and math.isfinite(value)


def metrics(ranked, judgments):
    def dcg(grades):
        return sum(
            (2**grade - 1) / math.log2(rank + 2)
            for rank, grade in enumerate(grades[:10])
        )

    ideal = dcg(sorted(judgments.values(), reverse=True))
    positives = {identifier for identifier, grade in judgments.items() if grade > 0}
    require(ideal > 0, "This audit requires answerable, sparsely judged queries")
    return dict(
        ndcg_at_10=dcg([judgments.get(identifier, 0) for identifier in ranked]) / ideal,
        judged_recall_at_10=len(positives.intersection(ranked)) / len(positives),
        no_useful_result=not bool(positives.intersection(ranked)),
    )


def quantile(values, probability):
    values = sorted(values)
    offset = (len(values) - 1) * probability
    low = int(offset)
    return values[low] + (values[math.ceil(offset)] - values[low]) * (offset - low)


def verify(evidence):
    require(evidence.get("schema_version") == 1, "Unsupported evidence version")
    require(evidence["conditions"]["k"] == 10, "Expected top-ten evidence")
    claims = evidence["source_protocol_claims"]
    require(
        claims["valid_comparison"] is True
        and not claims["failures"]
        and all(type(n) is int and n == 0 for n in claims["violations"].values()),
        "Source did not qualify a complete campaign",
    )
    queries = {}
    for query in evidence["queries"]:
        require(query["id"] not in queries, "Duplicate query")
        require(
            query["split"] in ("development", "test")
            and query["judgments_complete"] is False,
            "Expected separate splits with sparse judgments",
        )
        require(
            query["judgments"]
            and all(
                isinstance(k, str) and type(v) is int and 0 <= v <= 3
                for k, v in query["judgments"].items()
            ),
            "Invalid relevance grades",
        )
        queries[query["id"]] = query
    for split, expected in evidence["expected_queries_by_split"].items():
        require(
            sum(q["split"] == split for q in queries.values()) == expected,
            "Wrong query count",
        )
    rows = {}
    checked = {}
    for row in evidence["rows"]:
        key = row["query_id"], row["mode"]
        require(
            key not in rows and row["query_id"] in queries and row["mode"] in MODES,
            "Duplicate, unknown query or unsupported method",
        )
        query = queries[row["query_id"]]
        require(row["split"] == query["split"], "Query crossed its declared split")
        require(
            row["ranking_version"] == PROFILES[row["mode"]],
            "Unexpected method identity",
        )
        require(
            len(row["ranked"]) <= 10
            and all(isinstance(d, str) for d in row["ranked"])
            and len(set(row["ranked"])) == len(row["ranked"]),
            "Invalid or duplicated ranking",
        )
        require(
            len(row["samples"]) == evidence["conditions"]["repetitions"],
            "Wrong sample count",
        )
        require(
            all(
                number(sample[k]) and sample[k] >= 0
                for sample in row["samples"]
                for k in ("retrieval_ms", "http_ms", "response_bytes")
            ),
            "Invalid measurement",
        )
        calculated = metrics(row["ranked"], query["judgments"])
        for name in ("ndcg_at_10", "judged_recall_at_10"):
            require(
                close(calculated[name], row["metrics"][name]),
                "Per-query metric mismatch",
            )
        require(
            row["metrics"]["recall_at_10"] is None
            and row["metrics"]["judgments_complete"] is False
            and row["metrics"]["answerable"] is True
            and row["metrics"]["returned_on_unanswerable"] is None
            and row["metrics"]["no_useful_result"] == calculated["no_useful_result"],
            "Invalid sparse-judgment interpretation",
        )
        rows[key], checked[key] = row, calculated
    require(
        set(rows) == {(qid, mode) for qid in queries for mode in MODES},
        "Incomplete query/method matrix",
    )
    summary = {}
    for split in ("development", "test"):
        summary[split] = {}
        for mode in MODES:
            keys = [
                key
                for key, row in rows.items()
                if row["split"] == split and key[1] == mode
            ]
            require(keys, "Empty split")
            observed = evidence["summary"][split][mode]
            computed = {
                metric: statistics.mean(checked[key][metric] for key in keys)
                for metric in ("ndcg_at_10", "judged_recall_at_10")
            }
            computed["queries"] = len(keys)
            computed["queries_without_judged_positive_at_10"] = sum(
                checked[key]["no_useful_result"] for key in keys
            )
            computed["retrieval_ms"] = {
                label: quantile(
                    [s["retrieval_ms"] for key in keys for s in rows[key]["samples"]], p
                )
                for label, p in (("p50", 0.5), ("p95", 0.95))
            }
            require(
                observed["queries"] == len(keys) and observed["recall_at_10"] is None,
                "Invalid aggregate query count or exhaustive recall claim",
            )
            for metric in ("ndcg_at_10", "judged_recall_at_10"):
                require(
                    close(observed[metric], computed[metric]),
                    "Aggregate metric mismatch",
                )
            require(
                close(
                    observed["answerable_no_useful_result_rate"],
                    computed["queries_without_judged_positive_at_10"] / len(keys),
                ),
                "No-useful-result rate mismatch",
            )
            for label in ("p50", "p95"):
                require(
                    close(
                        observed["retrieval_ms"][label], computed["retrieval_ms"][label]
                    ),
                    "Latency quantile mismatch",
                )
            summary[split][mode] = computed
    return queries, rows, checked, summary


def audit_cases(evidence, queries, rows):
    cases = {}
    for case in evidence["cases"]:
        key = case["query_id"], case["mode"]
        require(
            key not in cases and key in rows and case["ranked"] == rows[key]["ranked"],
            "Invalid case identity",
        )
        require(
            len(case["score_evidence"]) == len(case["ranked"]), "Incomplete case scores"
        )
        values = [part["score"] for part in case["score_evidence"]]
        require(
            all(number(v) for v in values)
            and all(a >= b for a, b in zip(values, values[1:])),
            "Unordered or non-finite case scores",
        )
        for score in case["score_evidence"]:
            require(number(score["score"]), "Invalid score")
            if key[1] == "hybrid":
                total = 0.0
                for channel in ("lexical", "semantic"):
                    part = score.get(channel)
                    if part is not None:
                        require(
                            type(part["rank"]) is int and 1 <= part["rank"] <= 100,
                            "Invalid RRF rank",
                        )
                        require(
                            number(part["score"])
                            and close(part["contribution"], 0.5 / (60 + part["rank"])),
                            "RRF contribution mismatch",
                        )
                        total += part["contribution"]
                require(close(score["score"], total), "Fused score mismatch")
        cases[key] = case
    require(
        {key[0] for key in cases} == set(evidence["expected_regression_cases"]),
        "Missing expected regression cases",
    )
    result = []
    for qid in sorted({key[0] for key in cases}):
        require(
            all((qid, mode) in cases for mode in MODES), "Incomplete regression case"
        )
        positive = [d for d, g in queries[qid]["judgments"].items() if g > 0]
        positions = {}
        for mode in MODES:
            ranked = cases[qid, mode]["ranked"]
            positions[mode] = {
                d: ranked.index(d) + 1 if d in ranked else None for d in positive
            }
        result.append(
            dict(
                query_id=qid,
                positive_documents=positive,
                returned_positions=positions,
                absent_position_meaning="Outside the returned top ten; full channel rank is not established by these artifacts.",
            )
        )
    return result


def compare_previous(previous, queries, rows):
    require(
        previous["valid_comparison"] is True and not previous["failures"],
        "Invalid previous campaign",
    )
    original = {(r["query_id"], r["method"]): r for r in previous["rows"]}
    require(len(original) == len(previous["rows"]), "Duplicate previous campaign pair")
    comparisons = {}
    for mode in MODES:
        changes, order, membership = [], [], []
        for (qid, kind), row in rows.items():
            if kind != mode or row["split"] != "test":
                continue
            require(
                (qid, METHODS[mode]) in original, "Missing previous query/method pair"
            )
            old = original[qid, METHODS[mode]]
            require(
                len(old["ranked"]) <= 10
                and len(set(old["ranked"])) == len(old["ranked"])
                and len(old["scores"]) == len(old["ranked"]),
                "Invalid previous ranking",
            )
            require(
                all(number(part["score"]) for part in old["scores"]),
                "Invalid previous score",
            )
            old_metric = metrics(old["ranked"], queries[qid]["judgments"])["ndcg_at_10"]
            require(
                close(old_metric, old["metrics"]["ndcg_at_10"]),
                "Previous metric cannot be reproduced with these judgments",
            )
            new_metric = metrics(row["ranked"], queries[qid]["judgments"])["ndcg_at_10"]
            if old["ranked"] != row["ranked"]:
                order.append(qid)
            if set(old["ranked"]) != set(row["ranked"]):
                membership.append(qid)
            if not close(old_metric, new_metric):
                old_scores = {
                    d: score["score"] for d, score in zip(old["ranked"], old["scores"])
                }
                same_tied_positions = set(old["ranked"]) == set(row["ranked"]) and all(
                    old_scores[a] == old_scores[b]
                    for a, b in zip(old["ranked"], row["ranked"])
                )
                changes.append(
                    dict(
                        query_id=qid,
                        previous_ndcg=old_metric,
                        current_ndcg=new_metric,
                        changed_only_within_previous_exact_ties=same_tied_positions,
                    )
                )
        comparisons[mode] = dict(
            changed_order_queries=sorted(order),
            changed_membership_queries=sorted(membership),
            material_ndcg_changes=changes,
        )
    return comparisons


def audit(evidence, previous=None):
    queries, rows, checked, summary = verify(evidence)
    comparisons = {}
    test_ids = [
        key[0]
        for key, row in rows.items()
        if row["split"] == "test" and key[1] == "hybrid"
    ]
    for baseline in ("lexical", "semantic"):
        deltas = [
            checked[q, "hybrid"]["ndcg_at_10"] - checked[q, baseline]["ndcg_at_10"]
            for q in test_ids
        ]
        rng = random.Random(evidence["conditions"]["seed"])
        resamples = [
            statistics.mean(rng.choices(deltas, k=len(deltas))) for _ in range(2000)
        ]
        interval = dict(
            mean=statistics.mean(deltas),
            low=quantile(resamples, 0.025),
            high=quantile(resamples, 0.975),
        )
        for k, v in interval.items():
            require(
                close(v, evidence["reported_paired_intervals"][baseline][k]),
                "Bootstrap interval mismatch",
            )
        counts = dict(
            wins=sum(v > 1e-12 for v in deltas),
            ties=sum(abs(v) <= 1e-12 for v in deltas),
            losses=sum(v < -1e-12 for v in deltas),
            rescued=sum(
                checked[q, baseline]["no_useful_result"]
                and not checked[q, "hybrid"]["no_useful_result"]
                for q in test_ids
            ),
            lost=sum(
                not checked[q, baseline]["no_useful_result"]
                and checked[q, "hybrid"]["no_useful_result"]
                for q in test_ids
            ),
        )
        reported = evidence["reported_comparisons"][baseline]
        for k in ("wins", "ties", "losses"):
            require(counts[k] == reported[k], "Paired comparison mismatch")
        require(
            counts["rescued"]
            == reported["rescued_queries_without_a_positive_judgment_at_10"]
            and counts["lost"]
            == reported["lost_queries_with_a_positive_judgment_at_10"],
            "Rescue/loss mismatch",
        )
        comparisons[baseline] = dict(
            counts=counts, exploratory_paired_interval=interval
        )
    result = dict(
        schema_version=1,
        scope="Offline audit of already exposed evidence, not a new retrieval run or profile qualification",
        metric_pairs_checked=len(rows),
        summary=summary,
        hybrid_minus_baseline=comparisons,
        regression_cases=audit_cases(evidence, queries, rows),
        source_tie_counts_not_independently_recomputed=dict(
            queries=evidence["reported_ties"]["queries_with_returned_ties"],
            judged_quality_sensitive=evidence["reported_ties"][
                "queries_with_judged_quality_sensitive_ties"
            ],
            reason="The curated rows do not include every hybrid score or candidates below rank ten.",
        ),
        new_http_calls=0,
        new_provider_calls=0,
        promotion_authorized=False,
    )
    if previous is not None:
        require(
            previous["model_space"] == evidence["model_space"],
            "Different embedding-space identities",
        )
        result["previous_campaign_ranking_comparison"] = compare_previous(
            previous, queries, rows
        )
    return result


def read(path):
    raw = path.read_bytes()

    def reject(value):
        raise ValueError("Non-finite JSON value: " + value)

    return json.loads(raw, parse_constant=reject), hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--previous", type=Path)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        parser.error("Report already exists; preserve prior evidence")
    evidence, digest = read(args.input)
    previous, previous_digest = read(args.previous) if args.previous else (None, None)
    result = audit(evidence, previous)
    result.update(input_sha256=digest, previous_sha256=previous_digest)
    with args.report.open("x") as stream:
        json.dump(result, stream, indent=2, allow_nan=False)
        stream.write("\n")
    print(
        f"Verified {result['metric_pairs_checked']} query/method pairs; wrote {args.report}"
    )


if __name__ == "__main__":
    main()
