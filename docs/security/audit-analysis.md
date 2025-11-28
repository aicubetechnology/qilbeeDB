# Audit Log Analysis Guide

This guide provides practical techniques for analyzing QilbeeDB audit logs to detect security threats, investigate incidents, and maintain compliance.

## Security Monitoring Scenarios

### 1. Brute Force Attack Detection

Detect potential brute force attacks by analyzing failed login patterns.

#### Using the API

```bash
# Get failed logins in the last hour
curl -X GET "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&limit=500" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events'
```

#### Using Python SDK

```python
from qilbeedb import QilbeeDB

db = QilbeeDB(host="localhost", port=7474, username="admin", password="your-password")

# Get failed login attempts
failed_logins = db.get_failed_logins(limit=500)

# Analyze by IP address
from collections import Counter
ip_counts = Counter(event.get('ip_address') for event in failed_logins['events'])

# Flag IPs with more than 10 failures
suspicious_ips = {ip: count for ip, count in ip_counts.items() if count > 10}
for ip, count in suspicious_ips.items():
    print(f"ALERT: {ip} has {count} failed login attempts")
```

#### Using jq for Analysis

```bash
# Group failed logins by username
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&limit=1000" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events | group_by(.username) | .[] | {username: .[0].username, attempts: length}] | sort_by(.attempts) | reverse'

# Group by IP address
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&limit=1000" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events | group_by(.ip_address) | .[] | {ip: .[0].ip_address, attempts: length}] | sort_by(.attempts) | reverse'

# Find failed logins from same IP targeting multiple users
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&limit=1000" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events | group_by(.ip_address) | .[] | select(length > 5) | {ip: .[0].ip_address, targets: [.[].username] | unique}]'
```

### 2. Credential Stuffing Detection

Identify credential stuffing attacks where attackers try known username/password combinations.

```bash
# Find rapid succession of failed logins (potential automation)
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=login_failed&limit=1000" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events | sort_by(.timestamp) | .[] | {username, ip: .ip_address, time: .timestamp}]'
```

#### Python Analysis Script

```python
from datetime import datetime, timedelta
from collections import defaultdict

def detect_credential_stuffing(failed_logins, time_window_minutes=5, threshold=20):
    """Detect credential stuffing by finding rapid login attempts."""
    ip_timestamps = defaultdict(list)

    for event in failed_logins['events']:
        ip = event.get('ip_address')
        timestamp = datetime.fromisoformat(event['timestamp'].replace('Z', '+00:00'))
        ip_timestamps[ip].append(timestamp)

    alerts = []
    for ip, timestamps in ip_timestamps.items():
        timestamps.sort()
        for i, ts in enumerate(timestamps):
            window_end = ts + timedelta(minutes=time_window_minutes)
            count = sum(1 for t in timestamps if ts <= t <= window_end)
            if count >= threshold:
                alerts.append({
                    'ip': ip,
                    'count': count,
                    'window_start': ts.isoformat(),
                    'severity': 'HIGH'
                })
                break

    return alerts

# Usage
alerts = detect_credential_stuffing(db.get_failed_logins(limit=1000))
for alert in alerts:
    print(f"ALERT: Potential credential stuffing from {alert['ip']}: {alert['count']} attempts")
```

### 3. Account Lockout Monitoring

Monitor account lockouts to identify targeted attacks or misconfigured systems.

```python
# Get lockout events
lockouts = db.get_audit_logs(event_type="account_locked", limit=100)

# Analyze lockout patterns
for event in lockouts['events']:
    print(f"Account locked: {event.get('username')} from {event.get('ip_address')} at {event.get('timestamp')}")
```

### 4. Privilege Escalation Detection

Detect unauthorized privilege changes or suspicious role assignments.

```bash
# Monitor role changes
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=role_assigned&limit=100" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '.events[] | {user: .username, action: .action, metadata, time: .timestamp}'

# Find Admin role assignments
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=role_assigned&limit=100" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events[] | select(.metadata.role == "Admin" or .metadata.role == "SuperAdmin")]'
```

### 5. API Key Abuse Detection

Monitor API key usage patterns to detect compromised keys.

```bash
# List all API key creations
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=api_key_created&limit=100" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events'

# Find API key validation failures (potential compromised keys)
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=api_key_validation_failed&limit=100" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events'
```

#### Python Analysis

```python
def analyze_api_key_usage(db):
    """Analyze API key usage patterns."""
    # Get key creation events
    created = db.get_audit_logs(event_type="api_key_created", limit=500)

    # Get key usage events
    used = db.get_audit_logs(event_type="api_key_used", limit=1000)

    # Get validation failures
    failures = db.get_audit_logs(event_type="api_key_validation_failed", limit=500)

    # Analyze usage by key
    key_usage = defaultdict(lambda: {'success': 0, 'failure': 0, 'ips': set()})

    for event in used['events']:
        key_id = event.get('metadata', {}).get('key_id', 'unknown')
        key_usage[key_id]['success'] += 1
        key_usage[key_id]['ips'].add(event.get('ip_address'))

    for event in failures['events']:
        key_id = event.get('metadata', {}).get('key_id', 'unknown')
        key_usage[key_id]['failure'] += 1

    # Alert on suspicious patterns
    for key_id, stats in key_usage.items():
        # Many IPs using same key
        if len(stats['ips']) > 10:
            print(f"ALERT: API key {key_id} used from {len(stats['ips'])} different IPs")

        # High failure rate
        total = stats['success'] + stats['failure']
        if total > 10 and stats['failure'] / total > 0.5:
            print(f"ALERT: API key {key_id} has {stats['failure']}/{total} failures")
```

### 6. Permission Denial Analysis

Investigate access control issues by analyzing permission denials.

```bash
# Get all permission denials
curl -s "http://localhost:7474/api/v1/audit-logs?result=forbidden&limit=200" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events'

# Group denials by user
curl -s "http://localhost:7474/api/v1/audit-logs?result=forbidden&limit=500" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events | group_by(.username) | .[] | {user: .[0].username, denials: length, resources: [.[].resource] | unique}]'
```

### 7. Rate Limit Violation Analysis

Monitor rate limit violations to identify abuse or misconfigured clients.

```bash
# Get rate limit violations
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=rate_limit_exceeded&limit=200" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events'

# Group by IP
curl -s "http://localhost:7474/api/v1/audit-logs?event_type=rate_limit_exceeded&limit=500" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq '[.events | group_by(.ip_address) | .[] | {ip: .[0].ip_address, violations: length}] | sort_by(.violations) | reverse'
```

## Incident Investigation Workflow

### Step 1: Identify the Incident Timeframe

```bash
# Get events around a specific time
START_TIME="2025-01-15T10:00:00Z"
END_TIME="2025-01-15T11:00:00Z"

curl -s "http://localhost:7474/api/v1/audit-logs?start_time=$START_TIME&end_time=$END_TIME&limit=1000" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events'
```

### Step 2: Filter by Affected User

```bash
# Get all events for a specific user
USERNAME="compromised_user"
curl -s "http://localhost:7474/api/v1/audit-logs?username=$USERNAME&limit=500" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events | sort_by(.timestamp)'
```

### Step 3: Filter by Source IP

```bash
# Get all events from a suspicious IP
IP="192.168.1.100"
curl -s "http://localhost:7474/api/v1/audit-logs?ip_address=$IP&limit=500" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.events | sort_by(.timestamp)'
```

### Step 4: Create Incident Timeline

```python
def create_incident_timeline(db, username=None, ip_address=None, start_time=None, end_time=None):
    """Create a chronological timeline of security events."""
    params = {'limit': 1000}
    if username:
        params['username'] = username
    if ip_address:
        params['ip_address'] = ip_address
    if start_time:
        params['start_time'] = start_time
    if end_time:
        params['end_time'] = end_time

    events = db.get_audit_logs(**params)

    # Sort by timestamp
    timeline = sorted(events['events'], key=lambda x: x['timestamp'])

    print("=" * 80)
    print("INCIDENT TIMELINE")
    print("=" * 80)

    for event in timeline:
        print(f"\n[{event['timestamp']}]")
        print(f"  Event: {event['event_type']}")
        print(f"  User: {event.get('username', 'N/A')}")
        print(f"  IP: {event.get('ip_address', 'N/A')}")
        print(f"  Result: {event.get('result', 'N/A')}")
        if event.get('metadata'):
            print(f"  Details: {event['metadata']}")

    return timeline
```

## Compliance Reporting

### Daily Security Summary

```python
def generate_daily_security_summary(db, date):
    """Generate a daily security summary report."""
    start_time = f"{date}T00:00:00Z"
    end_time = f"{date}T23:59:59Z"

    report = {
        'date': date,
        'metrics': {}
    }

    # Successful logins
    logins = db.get_audit_logs(event_type="login", start_time=start_time, end_time=end_time, limit=10000)
    report['metrics']['successful_logins'] = logins['count']

    # Failed logins
    failed = db.get_audit_logs(event_type="login_failed", start_time=start_time, end_time=end_time, limit=10000)
    report['metrics']['failed_logins'] = failed['count']

    # Permission denials
    denials = db.get_audit_logs(result="forbidden", start_time=start_time, end_time=end_time, limit=10000)
    report['metrics']['permission_denials'] = denials['count']

    # Rate limit violations
    rate_limits = db.get_audit_logs(event_type="rate_limit_exceeded", start_time=start_time, end_time=end_time, limit=10000)
    report['metrics']['rate_limit_violations'] = rate_limits['count']

    # User management actions
    user_created = db.get_audit_logs(event_type="user_created", start_time=start_time, end_time=end_time, limit=1000)
    user_deleted = db.get_audit_logs(event_type="user_deleted", start_time=start_time, end_time=end_time, limit=1000)
    report['metrics']['users_created'] = user_created['count']
    report['metrics']['users_deleted'] = user_deleted['count']

    # API key changes
    keys_created = db.get_audit_logs(event_type="api_key_created", start_time=start_time, end_time=end_time, limit=1000)
    keys_revoked = db.get_audit_logs(event_type="api_key_revoked", start_time=start_time, end_time=end_time, limit=1000)
    report['metrics']['api_keys_created'] = keys_created['count']
    report['metrics']['api_keys_revoked'] = keys_revoked['count']

    return report
```

### Weekly Compliance Report

```bash
#!/bin/bash
# weekly_compliance_report.sh

ADMIN_TOKEN="your-admin-token"
BASE_URL="http://localhost:7474"
START_DATE=$(date -v-7d +%Y-%m-%dT00:00:00Z)
END_DATE=$(date +%Y-%m-%dT23:59:59Z)

echo "=========================================="
echo "Weekly Security Compliance Report"
echo "Period: $START_DATE to $END_DATE"
echo "=========================================="

echo ""
echo "Authentication Summary:"
echo "-----------------------"
LOGINS=$(curl -s "$BASE_URL/api/v1/audit-logs?event_type=login&start_time=$START_DATE&end_time=$END_DATE" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.count')
FAILED=$(curl -s "$BASE_URL/api/v1/audit-logs?event_type=login_failed&start_time=$START_DATE&end_time=$END_DATE" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.count')
echo "Successful Logins: $LOGINS"
echo "Failed Logins: $FAILED"

echo ""
echo "User Management:"
echo "----------------"
USERS_CREATED=$(curl -s "$BASE_URL/api/v1/audit-logs?event_type=user_created&start_time=$START_DATE&end_time=$END_DATE" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.count')
USERS_DELETED=$(curl -s "$BASE_URL/api/v1/audit-logs?event_type=user_deleted&start_time=$START_DATE&end_time=$END_DATE" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.count')
echo "Users Created: $USERS_CREATED"
echo "Users Deleted: $USERS_DELETED"

echo ""
echo "Access Control:"
echo "---------------"
DENIALS=$(curl -s "$BASE_URL/api/v1/audit-logs?result=forbidden&start_time=$START_DATE&end_time=$END_DATE" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.count')
echo "Permission Denials: $DENIALS"

echo ""
echo "Rate Limiting:"
echo "--------------"
RATE_LIMITS=$(curl -s "$BASE_URL/api/v1/audit-logs?event_type=rate_limit_exceeded&start_time=$START_DATE&end_time=$END_DATE" \
  -H "Authorization: Bearer $ADMIN_TOKEN" | jq '.count')
echo "Rate Limit Violations: $RATE_LIMITS"
```

## SIEM Integration

### Elasticsearch Export

```python
from elasticsearch import Elasticsearch
from datetime import datetime, timedelta

def export_to_elasticsearch(db, es_client, index_name="qilbeedb-audit"):
    """Export audit logs to Elasticsearch for SIEM integration."""
    # Get recent events (last 24 hours)
    start_time = (datetime.utcnow() - timedelta(hours=24)).isoformat() + "Z"

    events = db.get_audit_logs(start_time=start_time, limit=10000)

    for event in events['events']:
        # Add additional fields for SIEM
        event['@timestamp'] = event['timestamp']
        event['source'] = 'qilbeedb'
        event['log_type'] = 'security_audit'

        # Index to Elasticsearch
        es_client.index(
            index=f"{index_name}-{datetime.utcnow().strftime('%Y.%m.%d')}",
            document=event
        )

    print(f"Exported {len(events['events'])} events to Elasticsearch")

# Usage
es = Elasticsearch(['http://elasticsearch:9200'])
export_to_elasticsearch(db, es)
```

### Splunk HEC Integration

```python
import requests

def send_to_splunk(db, splunk_url, hec_token, source="qilbeedb", sourcetype="audit"):
    """Forward audit events to Splunk HTTP Event Collector."""
    from datetime import datetime, timedelta

    start_time = (datetime.utcnow() - timedelta(hours=1)).isoformat() + "Z"
    events = db.get_audit_logs(start_time=start_time, limit=5000)

    headers = {
        'Authorization': f'Splunk {hec_token}',
        'Content-Type': 'application/json'
    }

    for event in events['events']:
        payload = {
            'event': event,
            'source': source,
            'sourcetype': sourcetype,
            'time': datetime.fromisoformat(event['timestamp'].replace('Z', '+00:00')).timestamp()
        }

        response = requests.post(splunk_url, headers=headers, json=payload)
        if response.status_code != 200:
            print(f"Failed to send event: {response.text}")

    print(f"Sent {len(events['events'])} events to Splunk")

# Usage
send_to_splunk(db, 'https://splunk.company.com:8088/services/collector', 'your-hec-token')
```

## Alerting Rules

### Example Alert Conditions

| Alert | Condition | Severity |
|-------|-----------|----------|
| Brute Force Attack | > 10 failed logins from same IP in 5 minutes | Critical |
| Credential Stuffing | > 50 failed logins targeting different users in 10 minutes | Critical |
| Account Takeover | Successful login after multiple failures | High |
| Privilege Escalation | Admin role assigned to user | High |
| API Key Compromise | Same API key used from > 5 IPs | Medium |
| Rate Limit Abuse | > 100 rate limit violations in 1 hour | Medium |
| Suspicious Admin Activity | Admin actions outside business hours | Medium |

### Python Alerting Script

```python
def check_security_alerts(db):
    """Check for security alert conditions."""
    from datetime import datetime, timedelta

    alerts = []
    now = datetime.utcnow()
    five_min_ago = (now - timedelta(minutes=5)).isoformat() + "Z"
    one_hour_ago = (now - timedelta(hours=1)).isoformat() + "Z"

    # Check for brute force
    failed_logins = db.get_failed_logins(start_time=five_min_ago, limit=1000)
    ip_counts = {}
    for event in failed_logins['events']:
        ip = event.get('ip_address')
        ip_counts[ip] = ip_counts.get(ip, 0) + 1

    for ip, count in ip_counts.items():
        if count > 10:
            alerts.append({
                'type': 'BRUTE_FORCE',
                'severity': 'CRITICAL',
                'message': f'Potential brute force attack from {ip}: {count} failed logins',
                'ip': ip
            })

    # Check for rate limit abuse
    rate_limits = db.get_audit_logs(event_type="rate_limit_exceeded", start_time=one_hour_ago, limit=1000)
    if rate_limits['count'] > 100:
        alerts.append({
            'type': 'RATE_LIMIT_ABUSE',
            'severity': 'MEDIUM',
            'message': f'High rate limit violations: {rate_limits["count"]} in last hour'
        })

    return alerts

# Run checks
alerts = check_security_alerts(db)
for alert in alerts:
    print(f"[{alert['severity']}] {alert['type']}: {alert['message']}")
```

## Next Steps

- [Audit Logging](audit.md) - Core audit logging documentation
- [Production Security Checklist](production-checklist.md) - Deployment security checklist
- [Authentication](authentication.md) - Configure authentication
- [Rate Limiting](rate-limiting.md) - Configure rate limits
