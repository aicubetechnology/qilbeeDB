# Development Setup

Contributing to QilbeeDB requires setting up a Rust development environment.

## Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Git
- Build tools (gcc/clang, make)

## Clone Repository

```bash
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeedb
```

## Build

```bash
# Build all crates
cargo build

# Build in release mode
cargo build --release
```

## Run Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

## Run Server

```bash
cargo run --bin qilbeedb
```

## Project Structure

```
qilbeedb/
├── crates/
│   ├── qilbee-core/      # Core types and traits
│   ├── qilbee-storage/   # Storage engine
│   ├── qilbee-graph/     # Graph operations  
│   ├── qilbee-query/     # Query engine
│   ├── qilbee-memory/    # Memory engine
│   ├── qilbee-protocol/  # Protocol definitions
│   └── qilbee-server/    # Server implementation
├── sdks/
│   └── python/           # Python SDK
└── docs/                 # Documentation
```

## Next Steps

- Review [Code Style](style.md)
- Write [Tests](testing.md)
- Submit a Pull Request
