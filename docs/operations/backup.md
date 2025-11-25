# Backup & Recovery

Backup and restore QilbeeDB data.

## Snapshots

### Create Snapshot

```bash
# HTTP API
curl -X POST http://localhost:7474/admin/snapshot
```

Response:
```json
{
  "snapshot_id": "20240115-103000",
  "path": "/data/snapshots/20240115-103000",
  "size_bytes": 1073741824
}
```

### List Snapshots

```bash
curl http://localhost:7474/admin/snapshots
```

### Restore Snapshot

```bash
# Stop database
docker stop qilbeedb

# Restore snapshot
cp -r /data/snapshots/20240115-103000/* /data/

# Start database
docker start qilbeedb
```

## File System Backup

### Backup Data Directory

```bash
# Stop database
docker stop qilbeedb

# Backup data
tar -czf qilbeedb-backup-$(date +%Y%m%d).tar.gz /data/qilbeedb/

# Start database
docker start qilbeedb
```

### Restore from Backup

```bash
# Stop database
docker stop qilbeedb

# Restore data
tar -xzf qilbeedb-backup-20240115.tar.gz -C /

# Start database
docker start qilbeedb
```

## Online Backups

For minimal downtime, use snapshots:

```bash
# Create snapshot (database stays online)
curl -X POST http://localhost:7474/admin/snapshot

# Backup snapshot directory
cp -r /data/snapshots/latest /backup/
```

## Automated Backups

### Cron Job

```bash
# Add to crontab
0 2 * * * /usr/local/bin/qilbee-backup.sh

# qilbee-backup.sh
#!/bin/bash
BACKUP_DIR="/backup/qilbeedb"
DATE=$(date +%Y%m%d)

# Create snapshot
curl -X POST http://localhost:7474/admin/snapshot

# Backup to S3
aws s3 sync /data/snapshots/latest s3://my-backups/qilbeedb/$DATE/
```

## Backup Strategy

### Daily Backups
- Automated snapshots
- Retain for 7 days

### Weekly Backups
- Full file system backup
- Retain for 4 weeks

### Monthly Backups
- Archive to cold storage
- Retain for 12 months

## Disaster Recovery

### Recovery Time Objective (RTO)
Target: < 1 hour

### Recovery Point Objective (RPO)
Target: < 24 hours (daily backups)

### Recovery Steps

1. Provision new instance
2. Restore latest backup
3. Verify data integrity
4. Update DNS/load balancer
5. Resume operations

## Verification

Test restore procedures regularly:

```bash
# Restore to test instance
docker run -d --name qilbeedb-test \
  -v /backup/latest:/data \
  qilbeedb/qilbeedb:latest

# Verify data
curl http://localhost:7475/health
curl http://localhost:7475/admin/stats
```

## Next Steps

- Configure [Deployment](deployment.md)
- Set up [Monitoring](monitoring.md)
- Optimize [Performance](performance.md)
