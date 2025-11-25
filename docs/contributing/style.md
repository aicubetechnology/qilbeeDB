# Code Style

QilbeeDB follows standard Rust conventions and best practices.

## Rust Style Guide

### Formatting

Use `rustfmt` for consistent formatting:

```bash
# Format all code
cargo fmt

# Check formatting
cargo fmt -- --check
```

Configuration in `rustfmt.toml`:
```toml
max_width = 100
tab_spaces = 4
edition = "2021"
```

### Naming Conventions

- **Types**: `PascalCase`
  ```rust
  struct NodeId(u64);
  enum PropertyValue { ... }
  ```

- **Functions**: `snake_case`
  ```rust
  fn create_node(labels: Vec<String>) -> Result<Node>
  ```

- **Constants**: `SCREAMING_SNAKE_CASE`
  ```rust
  const MAX_BATCH_SIZE: usize = 1000;
  ```

- **Modules**: `snake_case`
  ```rust
  mod query_engine;
  mod storage_backend;
  ```

### Error Handling

Use `Result` for fallible operations:

```rust
// Good
pub fn get_node(&self, id: NodeId) -> Result<Node> {
    self.storage
        .get(id)
        .ok_or(Error::NodeNotFound(id))
}

// Bad: unwrap/expect in library code
pub fn get_node(&self, id: NodeId) -> Node {
    self.storage.get(id).unwrap()  // Don't do this!
}
```

### Documentation

Document public APIs with doc comments:

```rust
/// Creates a new node with the specified labels and properties.
///
/// # Arguments
///
/// * `labels` - A vector of label strings
/// * `properties` - A hashmap of property key-value pairs
///
/// # Examples
///
/// ```
/// let node = graph.create_node(
///     vec!["User".to_string()],
///     hashmap!{"name" => "Alice".into()}
/// )?;
/// ```
///
/// # Errors
///
/// Returns `Error::InvalidLabel` if labels are empty.
pub fn create_node(
    &mut self,
    labels: Vec<String>,
    properties: HashMap<String, PropertyValue>
) -> Result<Node> {
    // ...
}
```

## Code Organization

### Module Structure

```rust
// src/lib.rs
pub mod graph;
pub mod storage;
pub mod query;
pub mod memory;

// src/graph/mod.rs
mod node;
mod relationship;

pub use node::Node;
pub use relationship::Relationship;
```

### Imports

Group imports by category:

```rust
// Standard library
use std::collections::HashMap;
use std::sync::Arc;

// External crates
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// Internal modules
use crate::storage::Storage;
use crate::error::{Error, Result};
```

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_node() {
        let mut graph = Graph::new();
        let node = graph.create_node(
            vec!["User".to_string()],
            HashMap::new()
        ).unwrap();

        assert_eq!(node.labels, vec!["User"]);
    }
}
```

### Integration Tests

Place in `tests/` directory:

```rust
// tests/graph_operations.rs
use qilbeedb::Graph;

#[test]
fn test_create_and_query_nodes() {
    let mut graph = Graph::new();
    // ...
}
```

## Linting

Run Clippy for lint checks:

```bash
# Run Clippy
cargo clippy

# Fix warnings
cargo clippy --fix
```

Common Clippy lints:
- Prefer `if let` over `match` for single patterns
- Use `into()` instead of explicit conversions
- Avoid unnecessary clones

## Performance

### Avoid Unnecessary Allocations

```rust
// Good: Use references
fn process_nodes(&self, nodes: &[Node]) { ... }

// Bad: Unnecessary clone
fn process_nodes(&self, nodes: Vec<Node>) { ... }
```

### Use Appropriate Data Structures

```rust
// Good: HashMap for lookups
let mut index: HashMap<NodeId, Node> = HashMap::new();

// Bad: Vec for frequent lookups
let mut index: Vec<(NodeId, Node)> = Vec::new();
```

## Git Commit Messages

Follow conventional commits:

```
feat: Add support for variable-length paths
fix: Correct index selection in query planner
docs: Update API documentation for graph operations
test: Add integration tests for memory consolidation
refactor: Simplify relationship storage logic
perf: Optimize node scan with bloom filters
```

Format:
```
<type>(<scope>): <subject>

<body>

<footer>
```

## Pull Request Guidelines

1. Create feature branch from `main`
2. Write tests for new functionality
3. Update documentation
4. Run full test suite
5. Format code with `rustfmt`
6. Run `clippy` and fix warnings
7. Write clear PR description

## Next Steps

- Set up [Development Environment](setup.md)
- Write [Tests](testing.md)
- Read [Architecture](../architecture/overview.md)
