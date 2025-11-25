# Deployment

QilbeeDB can be deployed in various configurations depending on your requirements.

## Docker Deployment

### Single Instance

```bash
docker run -d \
  --name qilbeedb \
  -p 7474:7474 \
  -p 7687:7687 \
  -v qilbeedb-data:/data \
  qilbeedb/qilbeedb:latest
```

### Docker Compose

```yaml
version: '3.8'

services:
  qilbeedb:
    image: qilbeedb/qilbeedb:latest
    ports:
      - "7474:7474"
      - "7687:7687"
    volumes:
      - ./data:/data
      - ./config.toml:/etc/qilbeedb/config.toml
    environment:
      - QILBEE_LOG_LEVEL=info
    restart: unless-stopped
```

## Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: qilbeedb
spec:
  replicas: 1
  selector:
    matchLabels:
      app: qilbeedb
  template:
    metadata:
      labels:
        app: qilbeedb
    spec:
      containers:
      - name: qilbeedb
        image: qilbeedb/qilbeedb:latest
        ports:
        - containerPort: 7474
        - containerPort: 7687
        volumeMounts:
        - name: data
          mountPath: /data
      volumes:
      - name: data
        persistentVolumeClaim:
          claimName: qilbeedb-pvc
```

## Production Checklist

- [ ] Enable authentication
- [ ] Configure backups
- [ ] Set up monitoring
- [ ] Tune memory settings
- [ ] Enable TLS/SSL
- [ ] Configure resource limits

## Next Steps

- Set up [Monitoring](monitoring.md)
- Configure [Backups](backup.md)
- Tune [Performance](performance.md)
