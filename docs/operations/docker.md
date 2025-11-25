# Docker

Run QilbeeDB in Docker containers.

## Quick Start

```bash
docker run -d \
  --name qilbeedb \
  -p 7474:7474 \
  -p 7687:7687 \
  -v qilbeedb-data:/data \
  qilbeedb/qilbeedb:latest
```

## Docker Compose

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  qilbeedb:
    image: qilbeedb/qilbeedb:latest
    ports:
      - "7474:7474"  # HTTP
      - "7687:7687"  # Bolt
    volumes:
      - ./data:/data
      - ./config.toml:/etc/qilbeedb/config.toml
    environment:
      - QILBEE_LOG_LEVEL=info
    restart: unless-stopped
```

Start:
```bash
docker-compose up -d
```

## Environment Variables

```bash
QILBEE_LOG_LEVEL=info        # Logging level
QILBEE_HTTP_PORT=7474        # HTTP port
QILBEE_BOLT_PORT=7687        # Bolt port
QILBEE_DATA_PATH=/data       # Data directory
```

## Volume Mounts

```bash
# Data persistence
-v qilbeedb-data:/data

# Configuration
-v ./config.toml:/etc/qilbeedb/config.toml

# Logs
-v ./logs:/var/log/qilbeedb
```

## Build from Source

```bash
# Clone repository
git clone https://github.com/aicubetechnology/qilbeeDB.git
cd qilbeedb

# Build Docker image
docker build -t qilbeedb:local .

# Run
docker run -d -p 7474:7474 -p 7687:7687 qilbeedb:local
```

## Health Check

```bash
# Check if container is running
docker ps | grep qilbeedb

# Check health endpoint
curl http://localhost:7474/health
```

## Next Steps

- Configure [Deployment](deployment.md)
- Set up [Monitoring](monitoring.md)
- Configure [Backups](backup.md)
