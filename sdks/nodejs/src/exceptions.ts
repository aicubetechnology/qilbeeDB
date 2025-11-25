/**
 * QilbeeDB exceptions.
 */

/**
 * Base exception for all QilbeeDB errors.
 */
export class QilbeeDBError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'QilbeeDBError';
    Object.setPrototypeOf(this, QilbeeDBError.prototype);
  }
}

/**
 * Failed to connect to database.
 */
export class ConnectionError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'ConnectionError';
    Object.setPrototypeOf(this, ConnectionError.prototype);
  }
}

/**
 * Authentication failed.
 */
export class AuthenticationError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

/**
 * Query execution failed.
 */
export class QueryError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'QueryError';
    Object.setPrototypeOf(this, QueryError.prototype);
  }
}

/**
 * Transaction operation failed.
 */
export class TransactionError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'TransactionError';
    Object.setPrototypeOf(this, TransactionError.prototype);
  }
}

/**
 * Memory operation failed.
 */
export class MemoryError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'MemoryError';
    Object.setPrototypeOf(this, MemoryError.prototype);
  }
}

/**
 * Graph not found.
 */
export class GraphNotFoundError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'GraphNotFoundError';
    Object.setPrototypeOf(this, GraphNotFoundError.prototype);
  }
}

/**
 * Node not found.
 */
export class NodeNotFoundError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'NodeNotFoundError';
    Object.setPrototypeOf(this, NodeNotFoundError.prototype);
  }
}

/**
 * Relationship not found.
 */
export class RelationshipNotFoundError extends QilbeeDBError {
  constructor(message: string) {
    super(message);
    this.name = 'RelationshipNotFoundError';
    Object.setPrototypeOf(this, RelationshipNotFoundError.prototype);
  }
}
