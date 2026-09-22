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

# Legacy graph clients load on access so the verified consumer remains stdlib-only.
from importlib import import_module
from .consumer import (
    VerifiedMemoryConsumer,
    VerifiedCursor,
    ConsumerCheckpoint,
    ConsumptionResult,
    MemoryDelivery,
    MemoryChangeSink,
    ConsumerError,
    ConsumerAPIError,
    ConsumerProtocolError,
    ConsumerTransportError,
    ReconciliationRequired,
)
from .relation_consumer import (
    VerifiedRelationConsumer,
    RelationCursor,
    RelationCheckpoint,
    RelationConsumptionResult,
    RelationDelivery,
    RelationChangeSink,
)
from .consolidation import (
    ConsolidationClient,
    ConsolidationInput,
    ConsolidationResult,
    ConsolidationStopped,
    ExternalConsolidationWorker,
    SQLiteConsolidationJournal,
)

_LEGACY_EXPORTS = {
    "QilbeeDB": "client",
    **{name: "graph" for name in ("Graph", "Node", "Relationship")},
    **{
        name: "memory"
        for name in (
            "AgentMemory",
            "Episode",
            "MemoryConfig",
            "SemanticSearchResult",
            "HybridSearchResult",
        )
    },
    **{name: "query" for name in ("Query", "QueryResult")},
    **{
        name: "exceptions"
        for name in (
            "QilbeeDBError",
            "ConnectionError",
            "AuthenticationError",
            "QueryError",
            "TransactionError",
        )
    },
}


def __getattr__(name):
    module = _LEGACY_EXPORTS.get(name)
    if module is None:
        raise AttributeError("module 'qilbeedb' has no attribute " + repr(name))
    value = getattr(import_module("." + module, __name__), name)
    globals()[name] = value
    return value


def __dir__():
    return sorted(set(globals()) | set(__all__))


__all__ = [
    "ConsolidationClient",
    "ConsolidationInput",
    "ConsolidationResult",
    "ConsolidationStopped",
    "ExternalConsolidationWorker",
    "SQLiteConsolidationJournal",
    "VerifiedRelationConsumer",
    "RelationCursor",
    "RelationCheckpoint",
    "RelationConsumptionResult",
    "RelationDelivery",
    "RelationChangeSink",
    "VerifiedMemoryConsumer",
    "VerifiedCursor",
    "ConsumerCheckpoint",
    "ConsumptionResult",
    "MemoryDelivery",
    "MemoryChangeSink",
    "ConsumerError",
    "ConsumerAPIError",
    "ConsumerProtocolError",
    "ConsumerTransportError",
    "ReconciliationRequired",
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
