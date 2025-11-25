# Installation

QilbeeDB can be installed using Docker, built from source, or run as a standalone binary. Choose the method that best fits your deployment needs.

## Quick Start with Docker

The fastest way to get started with QilbeeDB is using Docker:

```bash
# Pull the latest QilbeeDB image
docker pull qilbeedb/qilbeedb:latest

# Run QilbeeDB
docker run -d \
  --name qilbeedb \
  -p 7474:7474 \
  -p 7687:7687 \
  -v qilbeedb-data:/data \
  qilbeedb/qilbeedb:latest
```

QilbeeDB will be available at:
- HTTP REST API: `http://localhost:7474`
- Bolt Protocol: `bolt://localhost:7687`

## Docker Compose

For production deployments with persistence and monitoring:

```yaml
version: '3.8'

services:
  qilbeedb:
    image: qilbeedb/qilbeedb:latest
    container_name: qilbeedb
    ports:
      - "7474:7474"  # HTTP
      - "7687:7687"  # Bolt
    volumes:
      - qilbeedb-data:/data
      - qilbeedb-logs:/logs
    environment:
      - QILBEE_LOG_LEVEL=info
      - QILBEE_STORAGE_PATH=/data
      - QILBEE_ENABLE_METRICS=true
    restart: unless-stopped

volumes:
  qilbeedb-data:
  qilbeedb-logs:
```

Save this as `docker-compose.yml` and run:

```bash
docker-compose up -d
```

## Building from Source

### Prerequisites

Before building QilbeeDB from source, ensure you have:

- **Rust** (1.70 or later) - [Install Rust](https://rustup.rs/)
- **Git** - Version control
- **Build tools** - C compiler (gcc/clang) and make

### Clone the Repository

```bash
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeedb
```

### Build QilbeeDB

```bash
# Build in release mode
cargo build --release

# The binary will be at: target/release/qilbee-server
```

### Run QilbeeDB

```bash
# Run with default configuration
./target/release/qilbee-server

# Run with custom configuration
./target/release/qilbee-server --config config.toml
```

## Platform-Specific Instructions

### macOS

```bash
# Install Rust if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install build dependencies
xcode-select --install

# Clone and build
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeedb
cargo build --release
```

### Linux (Ubuntu/Debian)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install build dependencies
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev

# Clone and build
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeedb
cargo build --release
```

### Linux (RHEL/CentOS/Fedora)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install build dependencies
sudo dnf groupinstall "Development Tools"
sudo dnf install -y openssl-devel

# Clone and build
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeedb
cargo build --release
```

## Installing the Python SDK

The Python SDK can be installed via pip:

```bash
# Install from PyPI (coming soon)
pip install qilbeedb

# Or install from source
cd sdks/python
pip install -e .
```

### Python Requirements

- Python 3.8 or later
- pip package manager

## Verifying Installation

### Test the Server

```bash
# Check if QilbeeDB is running
curl http://localhost:7474/health

# Expected response:
# {"status":"healthy","version":"0.1.0"}
```

### Test with Python SDK

```python
from qilbeedb import QilbeeDB

# Connect to QilbeeDB
db = QilbeeDB("http://localhost:7474")

# Create a test graph
graph = db.graph("test")

# Create a test node
node = graph.create_node(['Test'], {'message': 'Hello QilbeeDB!'})

print(f"Successfully created node with ID: {node.id}")
```

## Configuration

QilbeeDB can be configured using:

1. **Configuration file** (`config.toml`)
2. **Environment variables**
3. **Command-line arguments**

### Basic Configuration File

Create `config.toml`:

```toml
[server]
http_port = 7474
bolt_port = 7687
bind_address = "0.0.0.0"

[storage]
data_path = "/data"
enable_wal = true
sync_writes = true

[logging]
level = "info"
format = "json"

[metrics]
enabled = true
prometheus_port = 9090
```

### Environment Variables

```bash
# Server settings
export QILBEE_HTTP_PORT=7474
export QILBEE_BOLT_PORT=7687
export QILBEE_BIND_ADDRESS=0.0.0.0

# Storage settings
export QILBEE_STORAGE_PATH=/data
export QILBEE_ENABLE_WAL=true

# Logging
export QILBEE_LOG_LEVEL=info
export QILBEE_LOG_FORMAT=json

# Metrics
export QILBEE_ENABLE_METRICS=true
export QILBEE_PROMETHEUS_PORT=9090
```

## Next Steps

Now that QilbeeDB is installed, you can:

- Follow the [Quick Start Guide](quickstart.md) to build your first application
- Learn about [Configuration Options](configuration.md)
- Explore the [Python SDK](../client-libraries/python.md)
- Understand [Graph Operations](../graph-operations/nodes.md)

## Troubleshooting

### Port Already in Use

If ports 7474 or 7687 are already in use:

```bash
# Find process using port 7474
lsof -i :7474

# Use different ports
docker run -p 8474:7474 -p 8687:7687 qilbeedb/qilbeedb:latest
```

### Permission Denied (Linux)

If you get permission errors with data directory:

```bash
# Create data directory with proper permissions
sudo mkdir -p /var/lib/qilbeedb
sudo chown $(whoami):$(whoami) /var/lib/qilbeedb
```

### Build Errors

If you encounter build errors:

```bash
# Update Rust to latest version
rustup update

# Clean build artifacts and rebuild
cargo clean
cargo build --release
```

## Getting Help

- **Documentation**: You're reading it!
- **GitHub Issues**: [Report bugs or request features](https://github.com/aicubetechnology/qilbeeDB/issues)
- **Community**: Join our discussions on GitHub
