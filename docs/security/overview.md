# Security Overview

QilbeeDB includes enterprise-grade security features designed for production deployments. The security system provides comprehensive protection through multiple layers of defense.

## Security Features

### Authentication
- **JWT (JSON Web Tokens)** - RS256 algorithm for stateless authentication
- **API Keys** - SHA-256 hashed tokens with custom prefixes
- **Session Management** - Configurable expiration and inactivity timeouts

### Rate Limiting
- **Token Bucket Algorithm** - Smooth rate control with burst allowance
- **Per-Endpoint Policies** - Different limits for different operations
- **Global Protection** - Applied to all 24 API endpoints
- **Dynamic Management** - Modify limits at runtime via API

### Authorization
- **Role-Based Access Control (RBAC)** - Fine-grained permission system
- **5 Predefined Roles** - Read, Developer, Analyst, Admin, SuperAdmin
- **30+ Permissions** - Granular control over all operations
- **Custom Roles** - Create roles with specific permission sets

### Password Security
- **Argon2id Hashing** - Memory-hard algorithm resistant to GPU attacks
- **Unique Salts** - Each password gets a unique salt
- **Strong Password Requirements** - Enforced complexity rules
- **Password Rotation** - API support for password updates

### Audit & Compliance
- **Bi-Temporal Audit Log** - Track all security events with valid and transaction time
- **Event Filtering** - Query by user, action, result, and time range
- **Retention Policies** - Automatic cleanup of old events
- **IP and User-Agent Tracking** - Full request context logging

### Secure Bootstrap
- **Automatic Initial Setup** - Smart detection of first deployment
- **Interactive & Non-Interactive Modes** - Works in all environments
- **Environment Variable Support** - Docker/Kubernetes ready
- **State Tracking** - Prevents re-initialization

## Security Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    HTTP/Bolt Request                     │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │  Rate Limiting        │
           │  - Token Bucket Check │
           │  - Per-Endpoint Policy│
           │  - 429 if exceeded    │
           └───────────┬───────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │  Authentication       │
           │  - JWT Validation     │
           │  - API Key Check      │
           │  - Session Verify     │
           └───────────┬───────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │  Authorization (RBAC) │
           │  - Permission Check   │
           │  - Role Validation    │
           └───────────┬───────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │  Audit Logging        │
           │  - Event Recording    │
           │  - Bi-Temporal Store  │
           └───────────┬───────────┘
                       │
                       ▼
           ┌───────────────────────┐
           │  Request Processing   │
           └───────────────────────┘
```

## Security Layers

### Layer 1: Network Security
- TLS/SSL encryption for all connections
- Configurable bind addresses
- Port-based access control

### Layer 2: Rate Limiting
- Token bucket algorithm for smooth traffic control
- Per-endpoint rate limit policies
- User and IP-based tracking
- Automatic 429 response when exceeded

### Layer 3: Authentication
- Multiple authentication methods (JWT, API Keys)
- Token expiration and refresh
- Session timeout management

### Layer 4: Authorization
- Role-based permission checking
- Resource-level access control
- Operation-specific permissions

### Layer 5: Audit
- Comprehensive event logging
- Bi-temporal audit trail
- Tamper-evident records

## Default Roles

| Role | Permissions | Use Case |
|------|------------|----------|
| **Read** | Read nodes, relationships, properties | Read-only access |
| **Developer** | Read + Create, Update, Delete | Application development |
| **Analyst** | Read + Query execution | Data analysis |
| **Admin** | All operations + User management | Database administration |
| **SuperAdmin** | Full system control | System administration |

## Getting Started

1. **[Bootstrap Setup](bootstrap.md)** - Initial admin account creation
2. **[Authentication](authentication.md)** - Configure auth methods
3. **[Authorization](authorization.md)** - Set up RBAC
4. **[Rate Limiting](rate-limiting.md)** - Configure rate limits
5. **[Audit Logging](audit.md)** - Enable audit trails

## Security Best Practices

!!! warning "Production Deployment"
    Always enable authentication in production:
    ```bash
    # Enable auth in server config
    auth_enabled: true
    ```

!!! tip "Password Policy"
    Enforce strong passwords:
    - Minimum 12 characters
    - Mixed case letters
    - Numbers and special characters

!!! info "API Key Rotation"
    Rotate API keys regularly:
    ```bash
    # Revoke old key
    curl -X DELETE /api/v1/keys/{key_id}

    # Generate new key
    curl -X POST /api/v1/keys
    ```

## Configuration Example

```yaml
# Server configuration with security
server:
  auth_enabled: true

security:
  # Session configuration
  session_duration_secs: 86400  # 24 hours
  inactive_timeout_mins: 30

  # Rate limiting (global protection for all endpoints)
  rate_limiting:
    enabled: true
    login:
      max_requests: 100
      window_secs: 60
    api_key_creation:
      max_requests: 100
      window_secs: 60
    user_management:
      max_requests: 1000
      window_secs: 60
    general_api:
      max_requests: 100000
      window_secs: 60

  # Audit logging
  audit:
    enabled: true
    retention_days: 90
```

## Environment Variables

For automated deployments:

```bash
# Required for initial bootstrap
export QILBEEDB_ADMIN_EMAIL=admin@company.com
export QILBEEDB_ADMIN_PASSWORD=SecurePassword123!

# Optional
export QILBEEDB_ADMIN_USERNAME=admin
```

## Next Steps

- [Bootstrap & Initial Setup](bootstrap.md) - Set up your first admin account
- [Authentication Guide](authentication.md) - Configure authentication methods
- [Authorization (RBAC)](authorization.md) - Set up roles and permissions
- [Rate Limiting](rate-limiting.md) - Configure rate limit policies
- [Audit Logging](audit.md) - Enable and query audit logs
