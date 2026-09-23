"""Reconstruct graph comparison aggregates from already verified query rows."""
import statistics
from evaluate_retrieval import paired_interval
from evaluate_graph_retrieval import summary


def categories(rows, protocol, queries):
    return {
        category: {
            method: summary([
                row for row in rows
                if row["method"] == method and row["category"] == category
            ])
            for method in protocol["methods"]
        }
        for category in sorted({query["category"] for query in queries.values()})
    }


def comparisons(rows, protocol, queries):
    lookup = {(row["query_id"], row["method"]): row for row in rows}
    primary = protocol["primary_comparison"]
    result = {}
    for baseline in ("lexical", "semantic", "weighted_rrf_v1", "weighted_rrf_v2"):
        values = {}
        for metric in ("ndcg_at_10", "judged_recall_at_10", "all_labeled_supports_at_10"):
            deltas = [
                float(lookup[(qid, primary["candidate"])]["metrics"][metric])
                - float(lookup[(qid, baseline)]["metrics"][metric])
                for qid in sorted(queries)
            ]
            values[metric] = {
                "mean_delta": statistics.mean(deltas),
                "paired_interval": paired_interval(deltas, protocol["seed"]),
                "wins": sum(delta > 1e-12 for delta in deltas),
                "losses": sum(delta < -1e-12 for delta in deltas),
                "ties": sum(abs(delta) <= 1e-12 for delta in deltas),
            }
        result[baseline] = {
            "primary_predeclared": baseline == primary["baseline"],
            "metrics": values,
        }
    return result


def seed_comparisons(rows, protocol, queries):
    """Paired seed effects within the same graph profile and database import."""
    lookup = {(row["query_id"], row["method"]): row for row in rows}
    result = []
    for pair in protocol["seed_comparisons"]:
        values = {}
        for metric in ("ndcg_at_10", "judged_recall_at_10", "all_labeled_supports_at_10"):
            deltas = [float(lookup[qid, pair["candidate"]]["metrics"][metric])
                - float(lookup[qid, pair["baseline"]]["metrics"][metric]) for qid in sorted(queries)]
            values[metric] = {"mean_delta": statistics.mean(deltas),
                "paired_interval": paired_interval(deltas, protocol["seed"]),
                "wins": sum(d > 1e-12 for d in deltas),
                "losses": sum(d < -1e-12 for d in deltas),
                "ties": sum(abs(d) <= 1e-12 for d in deltas)}
        result.append(dict(pair, metrics=values))
    return result


def support_transitions(rows, protocol, queries):
    """Expose loss and recovery against positive judgments, not assumed relevance."""
    lookup = {}
    for row in rows:
        key = row["query_id"], row["method"]
        if key in lookup:
            raise ValueError("Duplicate query/method in support comparison")
        lookup[key] = row
    result = []
    for pair in protocol["support_comparisons"]:
        groups = {name: [] for name in (
            "no_positive_judgments", "baseline_no_labeled_support",
            "candidate_no_labeled_support", "lost_all_labeled_support",
            "gained_first_labeled_support", "lost_complete_support",
            "gained_complete_support",
        )}
        changes = []
        for qid in sorted(queries):
            if any((qid, pair[side]) not in lookup for side in ("baseline", "candidate")):
                raise ValueError("Missing query/method in support comparison")
            supports = {did for did, grade in queries[qid]["judgments"].items() if grade > 0}
            if not supports:
                groups["no_positive_judgments"].append(qid)
                continue
            try:
                base = supports & set(lookup[qid, pair["baseline"]]["ranked"])
                candidate = supports & set(lookup[qid, pair["candidate"]]["ranked"])
            except KeyError as exc:
                raise ValueError("Missing query/method in support comparison") from exc
            conditions = {
                "baseline_no_labeled_support": not base,
                "candidate_no_labeled_support": not candidate,
                "lost_all_labeled_support": bool(base) and not candidate,
                "gained_first_labeled_support": not base and bool(candidate),
                "lost_complete_support": base == supports and candidate != supports,
                "gained_complete_support": base != supports and candidate == supports,
            }
            for name, applies in conditions.items():
                if applies:
                    groups[name].append(qid)
            if base != candidate:
                changes.append({"query_id": qid, "lost": sorted(base - candidate),
                                "gained": sorted(candidate - base), "retained": sorted(base & candidate)})
        result.append(dict(pair, query_ids=groups, counts={k: len(v) for k, v in groups.items()},
                           changed_supports=changes))
    return result


def verify_support_transitions(report, rows, protocol, queries):
    expected = support_transitions(rows, protocol, queries)
    if report.get("support_transitions") != expected:
        raise ValueError("Support transitions differ from frozen judgments and rankings")
    return expected
