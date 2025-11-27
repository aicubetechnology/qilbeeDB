# Audit Logging

QilbeeDB provides comprehensive audit logging using a bi-temporal model to track all security-related events. This enables compliance, forensics, and security monitoring.

## Overview

The audit system tracks:

- **Authentication Events** - Login attempts, token validation, API key usage
- **Authorization Events** - Permission checks, access denials
- **User Management** - User creation, updates, deletion
- **API Key Management** - Key creation, revocation, usage
- **Rate Limiting** - Rate limit exceeded events
- **System Events** - Startup, shutdown, configuration changes

## Bi-Temporal Audit Model

Each audit event has two timestamps:

- **Event Time** (`timestamp`) - When the actual event occurred
- **Transaction Time** (`transaction_time`) - When the event was recorded in the system

This allows for:
- Historical analysis ("what happened at time X?")
- Audit trail reconstruction ("when did we learn about event Y?")
- Compliance reporting with temporal accuracy

## Configuration

The audit service is configured via `AuditConfig`:

```rust
pub struct AuditConfig {
    /// Maximum number of events to keep in memory (default: 100,000)
    pub max_events: usize,
    /// Number of days to retain events (default: 90)
    pub retention_days: i64,
    /// Whether to enable audit logging (default: true)
    pub enabled: bool,
    /// Path to audit log directory for file persistence (default: None - in-memory only)
    pub log_path: Option<PathBuf>,
    /// Maximum size of each log file in bytes before rotation (default: 10MB)
    pub max_file_size: u64,
}
```

### Configuration Options

| Option | Default | Description |
|--------|---------|-------------|
| `max_events` | 100,000 | Maximum events kept in memory |
| `retention_days` | 90 | Days to retain events before cleanup |
| `enabled` | true | Enable/disable audit logging |
| `log_path` | None | Directory for persistent JSONL log files |
| `max_file_size` | 10MB | Max file size before rotation |

### Example Configuration

```rust
// In-memory only (default)
let config = AuditConfig::default();

// With file persistence
let config = AuditConfig::with_file_logging("/var/log/qilbeedb/audit");

// Custom configuration
let config = AuditConfig {
    max_events: 50000,
    retention_days: 365,
    enabled: true,
    log_path: Some(PathBuf::from("/var/log/qilbeedb/audit")),
    max_file_size: 50 * 1024 * 1024, // 50MB
};
```

## Event Types Reference

### Authentication Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `login` | Successful login | User authenticates with username/password |
| `login_failed` | Failed login attempt | Invalid credentials provided |
| `logout` | User logout | User explicitly logs out |
| `token_refresh` | Token validation/refresh | JWT token validated in middleware |
| `token_refresh_failed` | Invalid token | JWT validation fails |

### API Key Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `api_key_created` | API key created | New API key generated |
| `api_key_revoked` | API key revoked | API key deleted/revoked |
| `api_key_used` | API key authentication | Request authenticated via X-API-Key |
| `api_key_validation_failed` | Invalid API key | X-API-Key header validation fails |

### User Management Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `user_created` | New user created | POST /api/v1/users |
| `user_updated` | User modified | PUT /api/v1/users/{id} |
| `user_deleted` | User deleted | DELETE /api/v1/users/{id} |
| `user_password_changed` | Password changed | Password update operation |

### Role Management Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `role_assigned` | Role assigned to user | Role added to user |
| `role_removed` | Role removed from user | Role removed from user |

### Authorization Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `permission_denied` | Access forbidden | User lacks required permission (403) |
| `access_granted` | Access allowed | Permission check passes |

### Rate Limiting Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `rate_limit_exceeded` | Rate limit hit | Request exceeds rate limit (429) |

### System Events

| Event Type | Description | Logged When |
|------------|-------------|-------------|
| `system_startup` | Server started | QilbeeDB server starts |
| `system_shutdown` | Server stopped | QilbeeDB server stops |
| `configuration_changed` | Config modified | Security configuration changed |

## Event Structure

Each audit event contains:

```json
{
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "event_type": "login",
  "timestamp": "2025-01-15T10:30:00Z",
  "transaction_time": "2025-01-15T10:30:01Z",
  "user_id": "usr_xyz789",
  "username": "alice",
  "action": "login",
  "resource": "authentication",
  "result": "success",
  "ip_address": "192.168.1.100",
  "user_agent": "Mozilla/5.0...",
  "metadata": {
    "key_id": "key_abc123"
  }
}
```

### Result Types

| Result | Description |
|--------|-------------|
| `success` | Operation completed successfully |
| `failure` | Operation failed (general failure) |
| `unauthorized` | Authentication failed (401) |
| `forbidden` | Authorization failed (403) |
| `error` | Internal error occurred |

## Query API

### Endpoint

```
GET /api/v1/audit-logs
```

**Access**: Admin and SuperAdmin roles only

### Query Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `event_type` | string | Filter by event type (e.g., `login`, `user_created`) |
| `user_id` | string | Filter by user ID |
| `username` | string | Filter by username |
| `result` | string | Filter by result (`success`, `failure`, `unauthorized`, `forbidden`) |
| `ip_address` | string | Filter by IP address |
| `start_time` | ISO8601 | Filter events after this time |
| `end_time` | ISO8601 | Filter events before this time |
| `limit` | integer | Maximum events to return (default: 100, max: 1000) |

### Response Format

```json
{
  "events": [
    {
      "event_id": "...",
      "event_type": "login",
      "timestamp": "2025-01-15T10:30:00Z",
      "username": "admin",
      "result": "success",
      ...
    }
  ],
  "count": 42,
  "limit": 100
}
```

### Query Examples

#### Get Recent Events

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs" \
  -H "Authorization: Bearer <admin-token>"
```

#### Filter by Event Type

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=login_failed" \
  -H "Authorization: Bearer <admin-token>"
```

#### Filter by User

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?username=alice" \
  -H "Authorization: Bearer <admin-token>"
```

#### Filter by Result

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?result=unauthorized" \
  -H "Authorization: Bearer <admin-token>"
```

#### Filter by Time Range

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?start_time=2025-01-01T00:00:00Z&end_time=2025-01-31T23:59:59Z" \
  -H "Authorization: Bearer <admin-token>"
```

#### Combined Filters with Limit

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&result=unauthorized&limit=50" \
  -H "Authorization: Bearer <admin-token>"
```

## Storage

### In-Memory Storage

By default, audit events are stored in-memory using a bounded queue (`VecDeque`):

- Fast queries for recent events
- Automatic eviction when `max_events` is reached
- Events lost on server restart

### File Persistence

When `log_path` is configured, events are also written to JSONL files:

- Append-only writes for tamper evidence
- Automatic file rotation by size
- Automatic cleanup of files older than `retention_days`
- Files named: `audit_YYYYMMDD_HHMMSS.jsonl`

### File Format

Each line is a JSON object:

```
{"event_id":"...","event_type":"login","timestamp":"2025-01-15T10:30:00Z",...}
{"event_id":"...","event_type":"user_created","timestamp":"2025-01-15T10:31:00Z",...}
```

## Compliance Queries

### Failed Login Attempts (Brute Force Detection)

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&limit=1000" \
  -H "Authorization: Bearer <admin-token>" \
  | jq 'group_by(.username) | map({username: .[0].username, count: length}) | sort_by(.count) | reverse'
```

### Admin Actions Report

```bash
# List all user management actions
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=user_created" \
  -H "Authorization: Bearer <admin-token>"

curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=user_deleted" \
  -H "Authorization: Bearer <admin-token>"
```

### Permission Denials

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?result=forbidden" \
  -H "Authorization: Bearer <admin-token>"
```

### API Key Activity

```bash
# Created API keys
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=api_key_created" \
  -H "Authorization: Bearer <admin-token>"

# Revoked API keys
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=api_key_revoked" \
  -H "Authorization: Bearer <admin-token>"
```

### Rate Limit Violations

```bash
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=rate_limit_exceeded" \
  -H "Authorization: Bearer <admin-token>"
```

## Best Practices

### Retention Policy

Set appropriate retention based on compliance requirements:

| Standard | Minimum Retention |
|----------|-------------------|
| GDPR | 6-12 months |
| HIPAA | 6 years |
| SOX | 7 years |
| PCI DSS | 1 year |

### Storage Planning

Audit logs can grow large:

- Monitor disk usage regularly
- Archive old logs to cold storage
- Use retention policies for automatic cleanup
- Configure appropriate `max_file_size` for rotation

### Regular Reviews

Schedule regular audit log reviews:

| Frequency | What to Review |
|-----------|----------------|
| Daily | Failed authentication attempts |
| Weekly | Permission denials, rate limit events |
| Monthly | User management actions, API key changes |
| Quarterly | Full compliance audit |

### Security Considerations

- Audit logs contain sensitive operation data
- Access restricted to Admin/SuperAdmin roles only
- Use file persistence for forensic evidence
- Append-only files prevent tampering
- Consider external log aggregation (SIEM) for production

## Integration Examples

### Python Client

```python
import requests

def get_audit_logs(token, event_type=None, limit=100):
    """Query audit logs via API."""
    headers = {"Authorization": f"Bearer {token}"}
    params = {"limit": limit}
    if event_type:
        params["event_type"] = event_type

    response = requests.get(
        "http://localhost:7474/api/v1/audit-logs",
        headers=headers,
        params=params
    )
    return response.json()

# Get failed logins
failed_logins = get_audit_logs(admin_token, event_type="login_failed")
print(f"Found {failed_logins['count']} failed login attempts")
```

### Log Aggregation (Splunk)

```python
import requests

def send_to_splunk(event):
    """Forward audit events to Splunk HEC."""
    requests.post(
        'https://splunk.company.com/services/collector',
        headers={'Authorization': 'Splunk your-hec-token'},
        json={'event': event}
    )
```

### Log Aggregation (Elasticsearch)

```python
from elasticsearch import Elasticsearch

es = Elasticsearch(['http://localhost:9200'])

def index_audit_event(event):
    """Index audit events in Elasticsearch."""
    es.index(
        index='qilbeedb-audit',
        document=event
    )
```

## Troubleshooting

### Missing Audit Events

If events aren't being logged:

1. Check if audit logging is enabled (`config.enabled = true`)
2. Verify the server was rebuilt after code changes
3. Check disk space availability (for file persistence)
4. Review server logs for errors

### High Storage Usage

If audit logs consume too much space:

1. Reduce `retention_days` value
2. Decrease `max_events` for memory
3. Reduce `max_file_size` for more frequent rotation
4. Archive old JSONL files to object storage

### Query Performance

For large audit logs:

1. Use specific filters to reduce result sets
2. Use time range filters to narrow scope
3. Keep `limit` parameter reasonable (< 1000)
4. Consider periodic cleanup via `cleanup()` method

## Next Steps

- [Authentication](authentication.md) - Configure auth to generate audit events
- [Authorization](authorization.md) - Set up RBAC to track permission checks
- [Rate Limiting](rate-limiting.md) - Configure rate limits that generate audit events
- [Security Overview](overview.md) - Complete security guide
