# Authentication

QilbeeDB supports multiple authentication methods to accommodate different use cases and deployment scenarios.

## Authentication Methods

### 1. JWT (JSON Web Tokens)

JSON Web Tokens provide stateless authentication using RS256 algorithm.

**Login to Get Token:**

```bash
curl -X POST http://localhost:7474/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username": "admin",
    "password": "your-password"
  }'
```

**Response:**

```json
{
  "access_token": "eyJhbGc....",
  "token_type": "Bearer",
  "expires_in": 86400
}
```

**Use Token:**

```bash
curl -X GET http://localhost:7474/api/v1/nodes \
  -H "Authorization: Bearer eyJhbGc...."
```

### 2. API Keys

API keys are long-lived credentials suitable for applications and services.

**Generate API Key:**

```bash
curl -X POST http://localhost:7474/api/v1/keys \
  -H "Authorization: Bearer your-jwt-token" \
  -d '{
    "name": "My Application",
    "expires_in_days": 365
  }'
```

**Response:**

```json
{
  "key": "qilbee_live_abc123...",
  "name": "My Application",
  "created_at": "2024-01-01T00:00:00Z",
  "expires_at": "2025-01-01T00:00:00Z"
}
```

**Use API Key:**

```bash
curl -X GET http://localhost:7474/api/v1/nodes \
  -H "X-API-Key: qilbee_live_abc123..."
```

## Session Management

### Session Configuration

```yaml
security:
  session_duration_secs: 86400  # 24 hours
  inactive_timeout_mins: 30      # 30 minutes inactivity
```

### Token Refresh

Refresh your JWT token before expiration:

```bash
curl -X POST http://localhost:7474/api/v1/auth/refresh \
  -H "Authorization: Bearer your-current-token"
```

### Logout

Invalidate your session:

```bash
curl -X POST http://localhost:7474/api/v1/auth/logout \
  -H "Authorization: Bearer your-token"
```

## Token Revocation

QilbeeDB supports token revocation to immediately invalidate tokens before they expire. This is essential for security scenarios like:

- User logout from all devices
- Compromised credential response
- Session termination after permission changes
- Emergency access revocation

### Revoke a Single Token

Revoke a specific JWT token to immediately invalidate it:

```bash
curl -X POST http://localhost:7474/api/v1/auth/revoke \
  -H "Content-Type: application/json" \
  -d '{
    "token": "eyJhbGc...."
  }'
```

**Response:**

```json
{
  "success": true,
  "message": "Token revoked successfully",
  "jti": "550e8400-e29b-41d4-a716-446655440000"
}
```

The `jti` (JWT ID) is returned for audit trail purposes.

### Revoke All Tokens for a User (Admin Only)

Administrators can revoke all tokens for a specific user, forcing them to re-authenticate:

```bash
curl -X POST http://localhost:7474/api/v1/auth/revoke-all \
  -H "Authorization: Bearer admin-token" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "550e8400-e29b-41d4-a716-446655440000",
    "reason": "security_incident"
  }'
```

**Response:**

```json
{
  "success": true,
  "message": "All tokens revoked for user",
  "user_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

!!! warning "Revoke All Tokens"
    This operation immediately invalidates ALL active sessions for the user.
    The user will need to log in again to obtain new tokens.

### Token Blacklist

Revoked tokens are stored in a persistent blacklist that is checked on every authenticated request. The blacklist:

- Persists across server restarts
- Automatically cleans up expired tokens
- Uses JWT ID (`jti`) for efficient lookup
- Supports both individual token and user-wide revocation

### Audit Trail

All token revocation events are logged in the audit system:

| Event Type | Description |
|------------|-------------|
| `token_revoked` | Single token was revoked |
| `all_tokens_revoked` | All tokens for a user were revoked |

Query revocation events:

```bash
curl -X GET "http://localhost:7474/api/v1/audit?event_type=token_revoked&limit=50" \
  -H "Authorization: Bearer admin-token"
```

## Rate Limiting

All API endpoints are protected by rate limiting to prevent abuse and brute-force attacks.

**Default Login Rate Limit:** 100 requests per minute

For detailed rate limiting configuration and management, see the [Rate Limiting](rate-limiting.md) documentation.

!!! info "Rate Limit Headers"
    Every API response includes rate limit headers:
    ```
    X-RateLimit-Limit: 100
    X-RateLimit-Remaining: 95
    X-RateLimit-Reset: 45
    ```

## Account Lockout

QilbeeDB automatically locks accounts after multiple failed login attempts to protect against brute-force attacks.

### How Lockout Works

| Setting | Default Value |
|---------|---------------|
| Max failed attempts | 5 |
| Initial lockout duration | 15 minutes |
| Lockout multiplier | 2x per subsequent lockout |
| Maximum lockout duration | 24 hours |

**Progressive Lockout:** Each subsequent lockout doubles in duration, up to 24 hours maximum.

- 1st lockout: 15 minutes
- 2nd lockout: 30 minutes
- 3rd lockout: 1 hour
- 4th lockout: 2 hours
- ... up to 24 hours

### Failed Login Response

When a login attempt fails, the response includes lockout tracking information:

```json
{
  "error": "Invalid username or password",
  "failed_attempts": 3,
  "remaining_attempts": 2
}
```

### Locked Account Response (HTTP 429)

When an account is locked, login attempts return HTTP 429 Too Many Requests:

```json
{
  "error": "Account locked due to too many failed login attempts",
  "locked": true,
  "lockout_expires": "2025-01-01T12:15:00Z",
  "lockout_remaining_seconds": 850
}
```

### Admin Lockout Management

Administrators can view and manage locked accounts through the lockout management API.

#### Get All Locked Accounts

```bash
curl -X GET http://localhost:7474/api/v1/lockouts \
  -H "Authorization: Bearer admin-token"
```

**Response:**

```json
{
  "count": 2,
  "locked_users": [
    ["user1", {
      "locked": true,
      "failed_attempts": 5,
      "lockout_count": 1,
      "lockout_expires": "2025-01-01T12:15:00Z",
      "lockout_remaining_seconds": 850,
      "remaining_attempts": 0
    }],
    ["user2", {
      "locked": true,
      "failed_attempts": 5,
      "lockout_count": 2,
      "lockout_expires": "2025-01-01T12:30:00Z",
      "lockout_remaining_seconds": 1750,
      "remaining_attempts": 0
    }]
  ]
}
```

#### Get Lockout Status for a User

```bash
curl -X GET http://localhost:7474/api/v1/lockouts/{username} \
  -H "Authorization: Bearer admin-token"
```

**Response:**

```json
{
  "username": "user1",
  "status": {
    "locked": true,
    "failed_attempts": 5,
    "lockout_count": 1,
    "lockout_expires": "2025-01-01T12:15:00Z",
    "lockout_remaining_seconds": 850,
    "remaining_attempts": 0
  }
}
```

#### Manually Lock an Account

Administrators can manually lock an account with a reason:

```bash
curl -X POST http://localhost:7474/api/v1/lockouts/{username}/lock \
  -H "Authorization: Bearer admin-token" \
  -H "Content-Type: application/json" \
  -d '{"reason": "Suspicious activity detected"}'
```

**Response:**

```json
{
  "success": true,
  "message": "Account 'user1' has been locked"
}
```

#### Unlock an Account

```bash
curl -X DELETE http://localhost:7474/api/v1/lockouts/{username} \
  -H "Authorization: Bearer admin-token"
```

**Response:**

```json
{
  "success": true,
  "message": "Account 'user1' has been unlocked"
}
```

### Python SDK

```python
from qilbeedb import QilbeeDB

# Connect as admin
db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")

# Get all locked accounts
locked = db.get_locked_accounts()
print(f"Locked accounts: {locked['count']}")

# Get lockout status for a specific user
status = db.get_lockout_status("user1")
print(f"User locked: {status['status']['locked']}")

# Manually lock an account
db.lock_account("user1", reason="Security review")

# Unlock an account
db.unlock_account("user1")
```

### Audit Events

Account lockout events are logged in the audit system:

| Event Type | Description |
|------------|-------------|
| `account_lockout_triggered` | Account locked after failed attempts |
| `account_locked` | Admin manually locked an account |
| `account_unlocked` | Account was unlocked (manual or time-based) |

Query lockout events:

```bash
curl -X GET "http://localhost:7474/api/v1/audit?event_type=account_lockout_triggered&limit=50" \
  -H "Authorization: Bearer admin-token"
```

## Password Requirements

!!! info "Strong Passwords"
    Passwords must meet these requirements:

    - Minimum 12 characters
    - At least one uppercase letter (A-Z)
    - At least one lowercase letter (a-z)
    - At least one number (0-9)
    - At least one special character (!@#$%^&*()_+-=[]{}|;:,.<>?)

**Valid Examples:**
- `MySecureP@ssw0rd`
- `Adm!n2024Password`
- `C0mplex!tyRul3s`

**Invalid Examples:**
- `shortpass` ❌ Too short, missing complexity
- `alllowercase123!` ❌ No uppercase
- `ALLUPPERCASE123!` ❌ No lowercase
- `NoDigitsHere!` ❌ No numbers
- `NoSpecialChar123` ❌ No special characters

## Password Management

### Change Password

```bash
curl -X PUT http://localhost:7474/api/v1/users/me/password \
  -H "Authorization: Bearer your-token" \
  -d '{
    "current_password": "old-password",
    "new_password": "NewSecureP@ssw0rd"
  }'
```

### Reset Password (Admin)

```bash
curl -X PUT http://localhost:7474/api/v1/users/{user_id}/password \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "new_password": "NewSecureP@ssw0rd"
  }'
```

## Best Practices

!!! tip "API Key Management"
    - Generate separate API keys for each application
    - Rotate keys regularly (every 90-365 days)
    - Revoke unused keys immediately
    - Store keys securely (environment variables, secrets managers)

!!! warning "Token Storage"
    - Never store JWT tokens in localStorage (XSS risk)
    - Use httpOnly cookies for web applications
    - Store API keys in environment variables, not in code

!!! info "Production Security"
    - Always use HTTPS in production
    - Enable rate limiting
    - Monitor failed login attempts
    - Set appropriate token expiration times

## Client Examples

### Python

```python
from qilbeedb import Client

# Option 1: Username/Password (gets JWT)
client = Client("http://localhost:7474")
client.login("admin", "password")

# Option 2: API Key
client = Client("http://localhost:7474", api_key="qilbee_live_...")

# Option 3: JWT Token
client = Client("http://localhost:7474", token="eyJhbGc...")
```

### JavaScript/Node.js

```javascript
const { Client } = require('@qilbeedb/client');

// Option 1: Username/Password
const client = new Client('http://localhost:7474');
await client.login('admin', 'password');

// Option 2: API Key
const client = new Client('http://localhost:7474', {
  apiKey: 'qilbee_live_...'
});

// Option 3: JWT Token
const client = new Client('http://localhost:7474', {
  token: 'eyJhbGc...'
});
```

### cURL

```bash
# Get token
TOKEN=$(curl -s -X POST http://localhost:7474/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"password"}' \
  | jq -r '.access_token')

# Use token
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:7474/api/v1/nodes
```

## Troubleshooting

### Invalid Token Error

```json
{
  "error": "Invalid or expired token",
  "message": "Token validation failed"
}
```

**Solutions:**
- Check if token is expired (refresh it)
- Verify token format (should start with `eyJ`)
- Ensure Authorization header format: `Bearer <token>`

### Account Locked

```json
{
  "error": "Account temporarily locked",
  "message": "Too many failed login attempts"
}
```

**Solutions:**
- Wait for lockout duration to expire
- Contact admin to unlock account
- Check for automated attacks on your account

## Next Steps

- [Authorization (RBAC)](authorization.md) - Configure permissions
- [Rate Limiting](rate-limiting.md) - Configure rate limit policies
- [Audit Logging](audit.md) - Track authentication events
- [Bootstrap Setup](bootstrap.md) - Initial admin account setup
