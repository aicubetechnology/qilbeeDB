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
