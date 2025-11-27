# Rate Limiting

QilbeeDB implements comprehensive rate limiting to protect against abuse, brute-force attacks, and denial-of-service attempts. Rate limiting is applied globally to all API endpoints with configurable policies per endpoint type.

## Overview

The rate limiting system provides:

- **Token Bucket Algorithm** - Smooth request distribution with burst allowance
- **Per-Endpoint Policies** - Different limits for different operations
- **User and IP-Based Limiting** - Tracks authenticated users and IP addresses
- **Dynamic Policy Management** - Modify limits at runtime via API
- **Response Headers** - Standard rate limit headers on all responses

## Default Rate Limits

| Endpoint Type | Max Requests | Window | Use Case |
|---------------|--------------|--------|----------|
| **Login** | 100 | 60 seconds | Brute-force protection |
| **API Key Management** | 100 | 60 seconds | Key generation protection |
| **User Management** | 1,000 | 60 seconds | User operations |
| **General API** | 100,000 | 60 seconds | Normal operations |

## Rate Limit Headers

All API responses include standard rate limit headers:

```http
HTTP/1.1 200 OK
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 95
X-RateLimit-Reset: 45
```

| Header | Description |
|--------|-------------|
| `X-RateLimit-Limit` | Maximum requests allowed in the current window |
| `X-RateLimit-Remaining` | Number of requests remaining in the current window |
| `X-RateLimit-Reset` | Seconds until the rate limit window resets |

## Rate Limit Exceeded Response

When a client exceeds the rate limit, the server returns HTTP 429:

```json
{
  "error": "Too Many Requests",
  "message": "Rate limit exceeded for Login",
  "limit": 100,
  "remaining": 0,
  "reset_in_seconds": 45
}
```

## Endpoint Types

### Login Endpoints

Most restrictive limits to prevent credential stuffing and brute-force attacks.

**Endpoints:**
- `POST /api/v1/auth/login`

**Default:** 100 requests per minute

### API Key Management

Moderate limits to prevent key abuse.

**Endpoints:**
- `POST /api/v1/api-keys`
- `GET /api/v1/api-keys`
- `DELETE /api/v1/api-keys/:id`

**Default:** 100 requests per minute

### User Management

Administrative operations with moderate limits.

**Endpoints:**
- `POST /api/v1/users`
- `GET /api/v1/users`
- `GET /api/v1/users/:id`
- `PUT /api/v1/users/:id`
- `DELETE /api/v1/users/:id`
- `PUT /api/v1/users/:id/roles`

**Default:** 1,000 requests per minute

### General API

High-throughput limit for normal database operations.

**Endpoints:**
- All graph operations (`/graphs/*`)
- All memory operations (`/memory/*`)
- Rate limit policy endpoints (`/api/v1/rate-limits/*`)
- Auth logout and refresh endpoints

**Default:** 100,000 requests per minute

## Rate Limit Policy Management API

Administrators can manage rate limit policies via the HTTP API.

### List All Policies

```bash
curl -X GET http://localhost:7474/api/v1/rate-limits \
  -H "Authorization: Bearer admin-token"
```

**Response:**

```json
{
  "policies": [
    {
      "id": "uuid-here",
      "name": "Login Rate Limit",
      "endpoint_type": "Login",
      "max_requests": 100,
      "window_secs": 60,
      "enabled": true,
      "created_at": "2024-01-01T00:00:00Z",
      "updated_at": "2024-01-01T00:00:00Z",
      "created_by": "admin-user-id"
    }
  ]
}
```

### Create Policy

```bash
curl -X POST http://localhost:7474/api/v1/rate-limits \
  -H "Authorization: Bearer admin-token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Custom API Limit",
    "endpoint_type": "GeneralApi",
    "max_requests": 50000,
    "window_secs": 60,
    "enabled": true
  }'
```

### Get Policy by ID

```bash
curl -X GET http://localhost:7474/api/v1/rate-limits/{policy_id} \
  -H "Authorization: Bearer admin-token"
```

### Update Policy

```bash
curl -X PUT http://localhost:7474/api/v1/rate-limits/{policy_id} \
  -H "Authorization: Bearer admin-token" \
  -H "Content-Type: application/json" \
  -d '{
    "max_requests": 200,
    "window_secs": 120,
    "enabled": true
  }'
```

### Delete Policy

```bash
curl -X DELETE http://localhost:7474/api/v1/rate-limits/{policy_id} \
  -H "Authorization: Bearer admin-token"
```

## Token Bucket Algorithm

QilbeeDB uses a token bucket algorithm for rate limiting:

```
┌─────────────────────────────────────┐
│           Token Bucket              │
│                                     │
│   ┌─────────────────────────────┐   │
│   │  Tokens: 95/100             │   │
│   │  Refill: 100 tokens/minute  │   │
│   │  Burst: Up to max capacity  │   │
│   └─────────────────────────────┘   │
│                                     │
│   Request → Token consumed          │
│   No tokens → 429 Rate Limited      │
│                                     │
└─────────────────────────────────────┘
```

**How it works:**

1. Each client (user or IP) gets a token bucket
2. Each request consumes one token
3. Tokens refill at a constant rate
4. If no tokens available, request is rejected with 429
5. Unused tokens accumulate up to the maximum (burst capacity)

## Client Identification

Rate limits are tracked per client:

### Authenticated Users
For requests with valid JWT or API key, rate limits are tracked by user ID.

### Anonymous Requests
For unauthenticated requests, rate limits are tracked by IP address using:
- `X-Forwarded-For` header (first IP in chain)
- `X-Real-IP` header (fallback)
- Direct connection IP (final fallback)

## Python SDK Usage

```python
from qilbeedb import QilbeeDB

client = QilbeeDB("http://localhost:7474")
client.login("admin", "password")

# Rate limit policies (admin only)
policies = client.rate_limits.list()
for policy in policies:
    print(f"{policy['name']}: {policy['max_requests']}/{policy['window_secs']}s")

# Create custom policy
policy = client.rate_limits.create(
    name="Custom Login Limit",
    endpoint_type="Login",
    max_requests=50,
    window_secs=60,
    enabled=True
)

# Update policy
client.rate_limits.update(policy['id'], max_requests=75)

# Delete policy
client.rate_limits.delete(policy['id'])
```

## Best Practices

!!! tip "Production Configuration"
    - Set appropriate limits based on expected traffic
    - Monitor rate limit events in audit logs
    - Consider lower limits for login endpoints
    - Use higher limits for read-heavy workloads

!!! warning "Security Considerations"
    - Don't set login limits too high (prevents brute-force protection)
    - Consider IP-based limits for public APIs
    - Monitor for distributed attacks across multiple IPs
    - Enable audit logging to track rate limit violations

!!! info "Performance Impact"
    - Rate limiting adds minimal latency (<1ms)
    - Token buckets are stored in memory
    - No database calls for rate limit checks
    - Headers are always included in responses

## Configuration Example

```yaml
# Server configuration with rate limiting
security:
  rate_limiting:
    enabled: true

    # Login protection
    login:
      max_requests: 100
      window_secs: 60

    # API key management
    api_key_creation:
      max_requests: 100
      window_secs: 60

    # User management
    user_management:
      max_requests: 1000
      window_secs: 60

    # General API
    general_api:
      max_requests: 100000
      window_secs: 60
```

## Monitoring Rate Limits

### Audit Log Events

Rate limit violations are logged to the audit log:

```bash
curl -X GET "http://localhost:7474/api/v1/audit?action=rate_limit_exceeded" \
  -H "Authorization: Bearer admin-token"
```

### Response Header Monitoring

Monitor the `X-RateLimit-Remaining` header to detect clients approaching limits:

```python
import requests

response = requests.get(
    "http://localhost:7474/api/v1/users",
    headers={"Authorization": "Bearer token"}
)

remaining = int(response.headers.get("X-RateLimit-Remaining", 0))
limit = int(response.headers.get("X-RateLimit-Limit", 0))

if remaining < limit * 0.1:  # Less than 10% remaining
    print("Warning: Approaching rate limit")
```

## Troubleshooting

### Frequent 429 Errors

1. Check current rate limit policy for the endpoint
2. Verify you're not making duplicate requests
3. Consider batching operations where possible
4. Request a higher limit if legitimate use case

### Rate Limits Not Applied

1. Verify the rate limit policy is enabled
2. Check that the server was restarted after policy changes
3. Ensure the endpoint path matches the policy type

### Inconsistent Remaining Values

1. Multiple server instances may have separate token buckets
2. Consider using a shared cache (Redis) for distributed deployments

## Next Steps

- [Authentication](authentication.md) - Configure authentication methods
- [Authorization (RBAC)](authorization.md) - Set up roles and permissions
- [Audit Logging](audit.md) - Track rate limit events
- [Security Overview](overview.md) - Complete security guide
