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


class TokenExpiredError(AuthenticationError):
    """JWT token has expired."""
    pass


class InvalidTokenError(AuthenticationError):
    """JWT token is invalid."""
    pass


class PermissionDeniedError(QilbeeDBError):
    """User lacks required permissions."""
    pass


class RateLimitError(QilbeeDBError):
    """Rate limit exceeded."""
    pass


class SecurityError(QilbeeDBError):
    """General security-related error."""
    pass


class NotFoundError(QilbeeDBError):
    """Resource not found (404)."""
    pass


class ValidationError(QilbeeDBError):
    """Request validation failed (400)."""
    pass


class ServerError(QilbeeDBError):
    """Server-side error (500)."""
    pass


class TimeoutError(QilbeeDBError):
    """Request timed out."""
    pass
