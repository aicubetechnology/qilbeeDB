"""
QilbeeDB Python SDK

Enterprise-grade Python client for QilbeeDB graph database with bi-temporal agent memory.

Example:
    >>> from qilbeedb import QilbeeDB
    >>> db = QilbeeDB("bolt://localhost:7687")
    >>> graph = db.graph("social")
    >>> alice = graph.create_node(["Person"], {"name": "Alice", "age": 30})
"""

__version__ = "0.2.0"
__author__ = "AICUBE TECHNOLOGY LLC"
__license__ = "Apache-2.0"

from .client import QilbeeDB
from .graph import Graph, Node, Relationship
from .memory import AgentMemory, Episode, MemoryConfig, SemanticSearchResult, HybridSearchResult
from .query import Query, QueryResult
from .exceptions import (
    QilbeeDBError,
    ConnectionError,
    AuthenticationError,
    QueryError,
    TransactionError,
)

__all__ = [
    "QilbeeDB",
    "Graph",
    "Node",
    "Relationship",
    "AgentMemory",
    "Episode",
    "MemoryConfig",
    "SemanticSearchResult",
    "HybridSearchResult",
    "Query",
    "QueryResult",
    "QilbeeDBError",
    "ConnectionError",
    "AuthenticationError",
    "QueryError",
    "TransactionError",
]
