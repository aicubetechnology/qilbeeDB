"""
Query builder and result handling.
"""

from typing import Any, Dict, List, Optional, TYPE_CHECKING

if TYPE_CHECKING:
    from .graph import Graph


class QueryStats:
    """Query execution statistics."""

    def __init__(
        self,
        nodes_created: int,
        nodes_deleted: int,
        relationships_created: int,
        relationships_deleted: int,
        execution_time_ms: float
    ):
        self.nodes_created = nodes_created
        self.nodes_deleted = nodes_deleted
        self.relationships_created = relationships_created
        self.relationships_deleted = relationships_deleted
        self.execution_time_ms = execution_time_ms


class QueryResult:
    """Query result container."""

    def __init__(self, results: List[Dict[str, Any]], stats: QueryStats):
        self.results = results
        self.stats = stats

    def __len__(self) -> int:
        return len(self.results)

    def __iter__(self):
        return iter(self.results)

    def __getitem__(self, index: int) -> Dict[str, Any]:
        return self.results[index]


class Query:
    """Fluent query builder for Cypher queries."""

    def __init__(self, graph: "Graph"):
        self.graph = graph
        self.query_parts = {
            "match": [],
            "where": [],
            "return": [],
            "order_by": [],
            "limit": None,
            "skip": None
        }
        self.parameters = {}

    def match(self, pattern: str) -> "Query":
        """Add MATCH clause."""
        self.query_parts["match"].append(pattern)
        return self

    def where(self, condition: str, params: Optional[Dict[str, Any]] = None) -> "Query":
        """Add WHERE clause."""
        self.query_parts["where"].append(condition)
        if params:
            self.parameters.update(params)
        return self

    def return_clause(self, *fields: str) -> "Query":
        """Add RETURN clause."""
        self.query_parts["return"].extend(fields)
        return self

    def order_by(self, field: str, desc: bool = False) -> "Query":
        """Add ORDER BY clause."""
        direction = "DESC" if desc else "ASC"
        self.query_parts["order_by"].append((field, direction))
        return self

    def limit(self, limit: int) -> "Query":
        """Add LIMIT clause."""
        self.query_parts["limit"] = limit
        return self

    def skip(self, skip: int) -> "Query":
        """Add SKIP clause."""
        self.query_parts["skip"] = skip
        return self

    def build(self) -> str:
        """Build the Cypher query."""
        parts = []

        # MATCH
        if self.query_parts["match"]:
            for match in self.query_parts["match"]:
                parts.append(f"MATCH {match}")

        # WHERE
        if self.query_parts["where"]:
            where_str = " AND ".join(self.query_parts["where"])
            parts.append(f"WHERE {where_str}")

        # RETURN
        if self.query_parts["return"]:
            return_str = ", ".join(self.query_parts["return"])
            parts.append(f"RETURN {return_str}")

        # ORDER BY
        if self.query_parts["order_by"]:
            order_strs = [f"{field} {direction}" for field, direction in self.query_parts["order_by"]]
            parts.append(f"ORDER BY {', '.join(order_strs)}")

        # SKIP
        if self.query_parts["skip"] is not None:
            parts.append(f"SKIP {self.query_parts['skip']}")

        # LIMIT
        if self.query_parts["limit"] is not None:
            parts.append(f"LIMIT {self.query_parts['limit']}")

        return " ".join(parts)

    def execute(self) -> QueryResult:
        """Execute the query."""
        cypher = self.build()
        return self.graph.query(cypher, self.parameters)
