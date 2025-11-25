"""
QilbeeDB exceptions.
"""


class QilbeeDBError(Exception):
    """Base exception for all QilbeeDB errors."""
    pass


class ConnectionError(QilbeeDBError):
    """Failed to connect to database."""
    pass


class AuthenticationError(QilbeeDBError):
    """Authentication failed."""
    pass


class QueryError(QilbeeDBError):
    """Query execution failed."""
    pass


class TransactionError(QilbeeDBError):
    """Transaction operation failed."""
    pass


class MemoryError(QilbeeDBError):
    """Memory operation failed."""
    pass


class GraphNotFoundError(QilbeeDBError):
    """Graph not found."""
    pass


class NodeNotFoundError(QilbeeDBError):
    """Node not found."""
    pass


class RelationshipNotFoundError(QilbeeDBError):
    """Relationship not found."""
    pass
