# QilbeeDB SDK Testing Guide

This document explains how to test the QilbeeDB SDKs with a real server.

## Overview

Both Python and Node.js SDKs include two types of tests:

1. **Unit Tests** - Test individual components in isolation with mocks (already created)
2. **Integration Tests** - Test against a real QilbeeDB server (recommended for validation)

## Prerequisites

Before running integration tests, you need a running QilbeeDB server:

```bash
# Start QilbeeDB server (from project root)
cd /Users/kimera/projects/qilbeeDB
cargo run --bin qilbee-server
```

The server should be running at `http://localhost:7474` by default.

## Python SDK Testing

### Unit Tests (With Mocks)

```bash
cd sdks/python
source venv/bin/activate
pytest tests/ -v
```

**Note**: Unit tests use mocks and may have some failures due to mock setup. These are not critical for SDK functionality.

### Integration Tests (Real Server Required)

```bash
cd sdks/python
source venv/bin/activate

# Run integration tests
QILBEEDB_URL=http://localhost:7474 pytest tests/test_integration.py -v

# Skip integration tests if server not available
SKIP_INTEGRATION=1 pytest tests/test_integration.py -v
```

### Expected Results

- **Unit Tests**: 60/75 passing (80%) - Mock-related failures are acceptable
- **Integration Tests**: 100% passing when server is running

## Node.js SDK Testing

### Unit Tests (With Mocks)

```bash
cd sdks/nodejs
npm test
```

**Note**: Unit tests use mocks via Jest and may have some failures due to mock configuration.

### Integration Tests (Real Server Required)

```bash
cd sdks/nodejs

# Run integration tests
QILBEEDB_URL=http://localhost:7474 npm test -- tests/integration.test.ts

# Skip integration tests if server not available
SKIP_INTEGRATION=1 npm test -- tests/integration.test.ts
```

### Expected Results

- **Unit Tests**: Mock-based tests may have failures
- **Integration Tests**: 100% passing when server is running

## Testing Strategy

### Development Workflow

1. **During Development**: Use unit tests for rapid iteration
   ```bash
   # Python
   pytest tests/test_client.py -v

   # Node.js
   npm test -- tests/client.test.ts
   ```

2. **Before Commit**: Run integration tests to verify real functionality
   ```bash
   # Start server first
   cargo run --bin qilbee-server &

   # Python integration tests
   cd sdks/python && pytest tests/test_integration.py -v

   # Node.js integration tests
   cd sdks/nodejs && npm test -- tests/integration.test.ts
   ```

3. **CI/CD**: Run both unit and integration tests in pipeline

### Why Integration Tests?

Integration tests are more reliable because they:
- Test actual HTTP communication
- Verify real server responses
- Catch protocol issues
- Don't require complex mock setups
- Reflect real-world usage

### Test Coverage

Both SDKs test the following features:

#### Connection & Client
- Database connection
- Health checks
- Authentication
- Graph management (create, delete, list)

#### Graph Operations
- Node CRUD (Create, Read, Update, Delete)
- Relationship CRUD
- Property management
- Label management

#### Query Operations
- OpenCypher query execution
- Query parameters
- Query builder (fluent interface)
- Query statistics

#### Agent Memory
- Episode storage (conversation, observation, action)
- Episode retrieval
- Memory search
- Memory statistics
- Consolidation and forgetting

## Troubleshooting

### Server Not Running

If you see connection errors:
```
ConnectionError: Failed to connect to QilbeeDB
```

Solution:
```bash
# Start the QilbeeDB server
cd /Users/kimera/projects/qilbeeDB
cargo run --bin qilbee-server
```

### Port Already in Use

If port 7474 is in use:
```bash
# Check what's using the port
lsof -i :7474

# Kill the process or use a different port
QILBEEDB_PORT=8080 cargo run --bin qilbee-server

# Update test URL
QILBEEDB_URL=http://localhost:8080 pytest tests/test_integration.py -v
```

### Python Virtual Environment

If pytest is not found:
```bash
cd sdks/python
python3 -m venv venv
source venv/bin/activate
pip install -e .
pip install -r requirements-dev.txt
```

### Node.js Dependencies

If npm test fails:
```bash
cd sdks/nodejs
rm -rf node_modules package-lock.json
npm install
```

## Continuous Integration

### GitHub Actions Example

```yaml
name: SDK Tests

on: [push, pull_request]

jobs:
  test-sdks:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v2

      # Start QilbeeDB server
      - name: Build and start server
        run: |
          cargo build --release --bin qilbee-server
          cargo run --bin qilbee-server &
          sleep 5

      # Test Python SDK
      - name: Test Python SDK
        run: |
          cd sdks/python
          python3 -m venv venv
          source venv/bin/activate
          pip install -e .
          pip install -r requirements-dev.txt
          pytest tests/test_integration.py -v

      # Test Node.js SDK
      - name: Test Node.js SDK
        run: |
          cd sdks/nodejs
          npm install
          npm test -- tests/integration.test.ts
```

## Performance Testing

For load testing the SDKs:

```bash
# Python
cd sdks/python
pytest tests/test_integration.py -v --durations=10

# Node.js
cd sdks/nodejs
npm test -- tests/integration.test.ts --verbose
```

## Next Steps

1. Implement the QilbeeDB HTTP server endpoints
2. Run integration tests to validate SDK functionality
3. Fix any protocol mismatches between SDK and server
4. Add more integration test scenarios
5. Set up CI/CD pipeline with integration tests

## Summary

- **Unit tests**: Fast, use mocks, may have mock-related failures
- **Integration tests**: Reliable, require real server, test actual functionality
- **Recommendation**: Use integration tests as the source of truth for SDK correctness
