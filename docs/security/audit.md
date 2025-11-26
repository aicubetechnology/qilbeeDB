# Audit Logging

QilbeeDB provides comprehensive audit logging using a bi-temporal model to track all security-related events. This enables compliance, forensics, and security monitoring.

## Overview

The audit system tracks:

- **Authentication Events** - Login attempts, token validation, API key usage
- **Authorization Events** - Permission checks, access denials
- **Data Access** - Read/write operations on sensitive data
- **Administrative Actions** - User management, role changes, config updates

## Bi-Temporal Audit Model

Each audit event has two timestamps:

- **Event Time** - When the actual event occurred
- **Transaction Time** - When the event was recorded in the system

This allows for:
- Historical analysis ("what happened at time X?")
- Audit trail reconstruction ("when did we learn about event Y?")
- Compliance reporting with temporal accuracy

## Event Structure

```json
{
  "id": "evt_abc123",
  "event_time": "2024-01-15T10:30:00Z",
  "transaction_time": "2024-01-15T10:30:01Z",
  "user_id": "usr_xyz789",
  "username": "alice",
  "action": "login",
  "result": "success",
  "ip_address": "192.168.1.100",
  "user_agent": "Mozilla/5.0...",
  "resource": "auth",
  "details": {
    "method": "jwt",
    "session_id": "ses_def456"
  }
}
```

## Audit Event Types

### Authentication Events

| Action | Description | Result Types |
|--------|-------------|--------------|
| `login` | User login attempt | success, unauthorized |
| `logout` | User logout | success |
| `validate_token` | JWT validation | success, unauthorized |
| `validate_api_key` | API key validation | success, unauthorized |
| `refresh_token` | Token refresh | success, unauthorized |

### Authorization Events

| Action | Description | Result Types |
|--------|-------------|--------------|
| `require_permission` | Permission check | success, forbidden |
| `assign_role` | Role assignment | success, forbidden |
| `remove_role` | Role removal | success, forbidden |

### Data Access Events

| Action | Description | Result Types |
|--------|-------------|--------------|
| `read_nodes` | Node read operation | success, forbidden |
| `create_nodes` | Node creation | success, forbidden |
| `update_nodes` | Node modification | success, forbidden |
| `delete_nodes` | Node deletion | success, forbidden |

### Administrative Events

| Action | Description | Result Types |
|--------|-------------|--------------|
| `create_user` | User creation | success, error |
| `update_user` | User modification | success, forbidden |
| `delete_user` | User deletion | success, forbidden |
| `configure_security` | Security config change | success, forbidden |

## Querying Audit Logs

### Get Recent Events

```bash
curl -X GET "http://localhost:7474/api/v1/audit/events?limit=100" \
  -H "Authorization: Bearer admin-token"
```

### Filter by User

```bash
curl -X GET "http://localhost:7474/api/v1/audit/events?user_id=usr_xyz789" \
  -H "Authorization: Bearer admin-token"
```

### Filter by Action

```bash
curl -X GET "http://localhost:7474/api/v1/audit/events?action=login" \
  -H "Authorization: Bearer admin-token"
```

### Filter by Result

```bash
curl -X GET "http://localhost:7474/api/v1/audit/events?result=unauthorized" \
  -H "Authorization: Bearer admin-token"
```

### Filter by Time Range

```bash
curl -X GET "http://localhost:7474/api/v1/audit/events?start=2024-01-01T00:00:00Z&end=2024-01-31T23:59:59Z" \
  -H "Authorization: Bearer admin-token"
```

### Combined Filters

```bash
curl -X GET "http://localhost:7474/api/v1/audit/events?action=login&result=unauthorized&limit=50" \
  -H "Authorization: Bearer admin-token"
```

## Audit Configuration

```yaml
security:
  audit:
    enabled: true
    retention_days: 90  # Keep logs for 90 days
    log_successful: true  # Log successful events
    log_failed: true  # Log failed events
    log_ip_address: true  # Record IP addresses
    log_user_agent: true  # Record user agents
```

## Compliance Queries

### Failed Login Attempts (Brute Force Detection)

```bash
# Find users with multiple failed logins
curl -X GET "http://localhost:7474/api/v1/audit/events?action=login&result=unauthorized&limit=1000" \
  -H "Authorization: Bearer admin-token" \
  | jq 'group_by(.username) | map({username: .[0].username, count: length}) | sort_by(.count) | reverse'
```

### Admin Actions Report

```bash
# List all administrative actions
curl -X GET "http://localhost:7474/api/v1/audit/events?action=create_user,update_user,delete_user,assign_role" \
  -H "Authorization: Bearer admin-token"
```

### Access Attempts by IP

```bash
# Group events by IP address
curl -X GET "http://localhost:7474/api/v1/audit/events" \
  -H "Authorization: Bearer admin-token" \
  | jq 'group_by(.ip_address) | map({ip: .[0].ip_address, count: length, users: [.[].username] | unique})'
```

### Permission Denials

```bash
# Find all permission denial events
curl -X GET "http://localhost:7474/api/v1/audit/events?result=forbidden" \
  -H "Authorization: Bearer admin-token"
```

## Real-Time Monitoring

### Event Stream (WebSocket)

```javascript
const ws = new WebSocket('ws://localhost:7474/api/v1/audit/stream');

ws.onmessage = (event) => {
  const auditEvent = JSON.parse(event.data);
  console.log('Audit Event:', auditEvent);

  // Alert on failed logins
  if (auditEvent.action === 'login' && auditEvent.result === 'unauthorized') {
    alert(`Failed login attempt for ${auditEvent.username}`);
  }
};
```

### Event Webhooks

Configure webhooks to receive audit events:

```bash
curl -X POST http://localhost:7474/api/v1/audit/webhooks \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "url": "https://your-monitoring-system.com/webhooks/audit",
    "events": ["login", "create_user", "configure_security"],
    "filters": {
      "result": ["unauthorized", "forbidden"]
    }
  }'
```

## Retention Management

### View Retention Policy

```bash
curl -X GET http://localhost:7474/api/v1/audit/retention \
  -H "Authorization: Bearer admin-token"
```

### Update Retention Policy

```bash
curl -X PUT http://localhost:7474/api/v1/audit/retention \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "retention_days": 365
  }'
```

### Manual Cleanup

```bash
# Delete events older than specified date
curl -X DELETE "http://localhost:7474/api/v1/audit/events?before=2023-01-01T00:00:00Z" \
  -H "Authorization: Bearer admin-token"
```

## Export Audit Logs

### Export to JSON

```bash
curl -X GET "http://localhost:7474/api/v1/audit/export?format=json&start=2024-01-01&end=2024-12-31" \
  -H "Authorization: Bearer admin-token" \
  > audit-2024.json
```

### Export to CSV

```bash
curl -X GET "http://localhost:7474/api/v1/audit/export?format=csv&start=2024-01-01&end=2024-12-31" \
  -H "Authorization: Bearer admin-token" \
  > audit-2024.csv
```

## Best Practices

!!! tip "Retention Policy"
    Set appropriate retention based on compliance requirements:

    - **GDPR**: 6-12 months minimum
    - **HIPAA**: 6 years
    - **SOX**: 7 years
    - **PCI DSS**: 1 year

!!! warning "Storage Planning"
    Audit logs can grow large:

    - Monitor disk usage regularly
    - Archive old logs to cold storage
    - Use retention policies to auto-cleanup
    - Consider log rotation strategies

!!! info "Regular Reviews"
    Schedule regular audit log reviews:

    - Daily: Failed authentication attempts
    - Weekly: Permission denials
    - Monthly: Administrative actions
    - Quarterly: Full compliance audit

## Integration Examples

### SIEM Integration

```python
# Send audit events to Splunk
import requests

def send_to_splunk(event):
    requests.post(
        'https://splunk.company.com/services/collector',
        headers={'Authorization': 'Splunk your-hec-token'},
        json={'event': event}
    )

# Configure webhook to call send_to_splunk
```

### Elasticsearch Integration

```python
# Index audit events in Elasticsearch
from elasticsearch import Elasticsearch

es = Elasticsearch(['http://localhost:9200'])

def index_audit_event(event):
    es.index(
        index='qilbeedb-audit',
        document=event
    )
```

### Grafana Dashboard

Query audit logs for visualization:

```sql
-- Failed logins over time
SELECT
  date_trunc('hour', event_time) as time,
  count(*) as failed_logins
FROM audit_events
WHERE action = 'login' AND result = 'unauthorized'
GROUP BY time
ORDER BY time DESC
```

## Troubleshooting

### Missing Audit Events

If events aren't being logged:

1. Check if audit logging is enabled
2. Verify retention policy hasn't deleted them
3. Check disk space availability
4. Review audit service logs

### High Storage Usage

If audit logs consume too much space:

1. Reduce retention period
2. Enable log rotation
3. Archive old logs to object storage
4. Filter less critical events

## Next Steps

- [Authentication](authentication.md) - Configure auth to generate audit events
- [Authorization](authorization.md) - Set up RBAC to track permission checks
- [Security Overview](overview.md) - Complete security guide
