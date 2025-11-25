# Testing

QilbeeDB uses comprehensive testing to ensure reliability.

## Test Types

### Unit Tests

Test individual functions and modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_creation() {
        let mut graph = Graph::new();
        let node = graph.create_node(
            vec!["User".to_string()],
            hashmap!{"name" => "Alice".into()}
        ).unwrap();

        assert_eq!(node.labels, vec!["User"]);
        assert_eq!(node.properties.get("name"), Some(&"Alice".into()));
    }

    #[test]
    fn test_invalid_label() {
        let mut graph = Graph::new();
        let result = graph.create_node(vec![], HashMap::new());

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidLabel));
    }
}
```

Run unit tests:
```bash
# All tests
cargo test

# Specific test
cargo test test_node_creation

# With output
cargo test -- --nocapture
```

### Integration Tests

Test multiple components together in `tests/`:

```rust
// tests/graph_operations.rs
use qilbeedb::{Graph, QueryBuilder};

#[test]
fn test_create_and_query() {
    let mut graph = Graph::new();

    // Create nodes
    let alice = graph.create_node(
        vec!["User".to_string()],
        hashmap!{"name" => "Alice".into(), "age" => 28.into()}
    ).unwrap();

    // Query
    let results = graph.query(
        "MATCH (u:User) WHERE u.age > $min_age RETURN u",
        hashmap!{"min_age" => 25.into()}
    ).unwrap();

    assert_eq!(results.len(), 1);
}
```

Run integration tests:
```bash
cargo test --test graph_operations
```

### Python SDK Tests

Test Python SDK in `sdks/python/tests/`:

```python
# tests/test_integration.py
import pytest
from qilbeedb import QilbeeDB

@pytest.fixture
def db():
    return QilbeeDB("http://localhost:7474")

def test_create_node(db):
    graph = db.graph("test_graph")
    
    node = graph.create_node(
        labels=['User'],
        properties={'name': 'Alice', 'age': 28}
    )
    
    assert node.id is not None
    assert 'User' in node.labels
    assert node.properties['name'] == 'Alice'
```

Run Python tests:
```bash
# In sdks/python/
pytest tests/
pytest tests/test_integration.py -v
```

## Test Coverage

Measure code coverage:

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html --output-dir coverage

# View report
open coverage/index.html
```

## Benchmarks

Performance benchmarks in `benches/`:

```rust
// benches/query_benchmark.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use qilbeedb::Graph;

fn benchmark_node_scan(c: &mut Criterion) {
    let mut graph = Graph::new();
    
    // Setup: Create 10k nodes
    for i in 0..10_000 {
        graph.create_node(
            vec!["User".to_string()],
            hashmap!{"id" => i.into()}
        ).unwrap();
    }

    c.bench_function("node_scan_10k", |b| {
        b.iter(|| {
            graph.query("MATCH (u:User) RETURN u", HashMap::new())
        })
    });
}

criterion_group!(benches, benchmark_node_scan);
criterion_main!(benches);
```

Run benchmarks:
```bash
cargo bench
```

## Test Data

Generate test data for integration tests:

```rust
// tests/common/mod.rs
pub fn create_social_network(graph: &mut Graph, size: usize) {
    // Create users
    let mut users = Vec::new();
    for i in 0..size {
        let user = graph.create_node(
            vec!["User".to_string()],
            hashmap!{
                "id" => i.into(),
                "name" => format!("User{}", i).into()
            }
        ).unwrap();
        users.push(user);
    }

    // Create friendships
    for i in 0..size {
        let friend_count = (i % 10) + 1;
        for j in 0..friend_count {
            let friend_id = (i + j + 1) % size;
            graph.create_relationship(
                users[i].id,
                "KNOWS".to_string(),
                users[friend_id].id,
                HashMap::new()
            ).unwrap();
        }
    }
}
```

## Continuous Integration

Tests run automatically on CI:

```yaml
# .github/workflows/test.yml
name: Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run tests
        run: cargo test --all
      - name: Run Python tests
        run: |
          cd sdks/python
          pip install -r requirements.txt
          pytest tests/
```

## Testing Best Practices

1. **Write Tests First** (TDD)
   - Write failing test
   - Implement feature
   - Verify test passes

2. **Test Edge Cases**
   ```rust
   #[test]
   fn test_empty_labels() { ... }
   
   #[test]
   fn test_null_properties() { ... }
   
   #[test]
   fn test_max_relationship_depth() { ... }
   ```

3. **Use Descriptive Names**
   ```rust
   // Good
   #[test]
   fn test_query_with_invalid_parameter_returns_error() { ... }

   // Bad
   #[test]
   fn test1() { ... }
   ```

4. **Mock External Dependencies**
   ```rust
   #[test]
   fn test_storage_failure() {
       let mock_storage = MockStorage::new();
       mock_storage.expect_get()
           .returning(|_| Err(Error::StorageError));
       
       let graph = Graph::new_with_storage(mock_storage);
       // Test error handling
   }
   ```

5. **Clean Up Test Data**
   ```rust
   #[test]
   fn test_with_cleanup() {
       let mut graph = Graph::new();
       // ... test code ...
       
       // Cleanup
       graph.delete_all().unwrap();
   }
   ```

## Test Organization

```
qilbeedb/
├── src/
│   └── lib.rs (unit tests)
├── tests/
│   ├── integration_tests.rs
│   ├── real_world_scenarios.rs
│   └── common/
│       └── mod.rs (shared test utilities)
├── benches/
│   └── query_benchmark.rs
└── sdks/
    └── python/
        └── tests/
            ├── test_integration.py
            └── test_real_world.py
```

## Running Tests

```bash
# All Rust tests
cargo test

# Specific test
cargo test test_node_creation

# Integration tests only
cargo test --test integration_tests

# Python tests
cd sdks/python && pytest

# With coverage
cargo tarpaulin

# Benchmarks
cargo bench
```

## Next Steps

- Follow [Code Style](style.md)
- Set up [Development Environment](setup.md)
- Read [Architecture](../architecture/overview.md)
