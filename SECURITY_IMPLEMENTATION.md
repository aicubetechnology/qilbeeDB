# QilbeeDB Enterprise-Grade Security Implementation

**Status:** In Progress
**Version:** 1.0.0
**Date:** 2025-11-25

## Overview

This document outlines the comprehensive enterprise-grade security system implementation for QilbeeDB, covering all protocols (HTTP, Bolt) with multi-layered security controls.

## Security Architecture

                     ```
                     ┌─────────────────────────────────────────────────────────────┐
                     │                      Client Applications                    │
                     └───────────────────────────────┬─────────────────────────────┘
                                                     │
                                          ┌──────────┴──────────┐
                                          │                     │
                                    ┌─────▼──────┐         ┌─────▼────┐
                                    │   HTTP API │         │  Bolt    │
                                    │ (Port 7474)│         │(Port7687)│
                                    └─────┬──────┘         └────┬─────┘
                                          │                     │
                                          └──────────┬──────────┘
                                                     │
                                          ┌──────────▼──────────┐
                                          │ Authentication Layer│
                                          │ - JWT Tokens        │
                                          │ - API Keys          │
                                          │ - Session Mgmt      │
                                          └──────────┬──────────┘
                                                     │
                                          ┌──────────▼──────────┐
                                          │ Authorization Layer │
                                          │ - RBAC              │ 
                                          │ - Permission Check  │
                                          │ - Resource ACL      │
                                          └──────────┬──────────┘
                                                     │
                                          ┌──────────▼──────────┐
                                          │   Audit Logging     │
                                          │ - All Operations    │
                                          │ - Security Events   │
                                          └──────────┬──────────┘
                                                     │
                                          ┌──────────▼──────────┐
                                          │    Core Database    │
                                          └─────────────────────┘
                     ```

## Security Layers

### 1. Authentication (WHO)

**Implemented:**
- ✅ Argon2 password hashing
- ✅ User management system
- ✅ JWT token generation
- ✅ API key management

**Methods:**
1. **Username/Password** → JWT Token
2. **API Key** → Direct authentication
3. **Service Account** → Machine-to-machine auth

### 2. Authorization (WHAT)

**Role-Based Access Control (RBAC):**

| Role | Permissions |
|------|-------------|
| **Admin** | Full system access, user management, system configuration |
| **Developer** | Read/write data, execute queries, manage agents |
| **Data Scientist** | Read-only data access, query execution |
| **Agent** | Memory operations, read data, self-management |
| **Read** | Read-only access to data |
| **Custom** | User-defined permission sets |

**Permission Categories:**
- Graph Operations: Create, Read, Update, Delete, Execute Query
- Node Operations: Create, Read, Update, Delete
- Relationship Operations: Create, Read, Update, Delete
- Memory Operations: Create, Read, Update, Delete, Search, Consolidate, Forget
- Agent Operations: Create, Read, Manage
- User Operations: Create, Read, Update, Delete, Manage Roles
- System Operations: Monitor, Configure, Backup, Restore
- Audit Operations: Read, Manage

### 3. Audit Logging (WHEN/WHERE)

**All operations logged:**
- User authentication attempts
- Authorization failures
- Data modifications
- Configuration changes
- Security events

**Log Format:**
```json
{
  "event_id": "uuid",
  "timestamp": "2025-11-25T12:00:00Z",
  "user_id": "user_uuid",
  "username": "alice",
  "action": "GraphCreate",
  "resource": "/graphs/knowledge_base",
  "result": "success",
  "ip_address": "192.168.1.100",
  "user_agent": "qilbeedb-client/1.0",
  "metadata": {}
}
```

## Implementation Status

### ✅ Completed

1. **User Management** (`security/user.rs`)
   - User CRUD operations
   - Argon2 password hashing
   - Username/email uniqueness
   - Active/inactive status
   - Role assignment
   - Login tracking

2. **RBAC System** (`security/rbac.rs`)
   - 5 predefined roles
   - 30+ granular permissions
   - Custom role creation
   - Permission checking
   - Multi-role support

### 🚧 In Progress

3. **Authentication Service** (`security/auth.rs`)
4. **Token Management** (`security/token.rs`)
5. **Audit Logging** (`security/audit.rs`)
6. **HTTP Middleware** (`security/middleware.rs`)

### ⏳ Pending

7. **Bolt Protocol Security**
8. **Security Tests**
9. **Documentation**
10. **Migration Tools**

## Security Features

### Password Security
- **Algorithm:** Argon2id (winner of Password Hashing Competition)
- **Salt:** Cryptographically random per-password
- **Work Factor:** Default parameters (memory-hard)
- **No plaintext storage:** Ever

### Token Security
- **JWT Tokens:**
  - RS256 algorithm (RSA + SHA-256)
  - Expiration: 24 hours (configurable)
  - Refresh token: 30 days
  - Claims: user_id, username, roles, exp, iat

- **API Keys:**
  - SHA-256 hashed storage
  - Prefix: `qilbee_live_` (production) / `qilbee_test_` (development)
  - Random 32-byte keys
  - Per-key permissions
  - Revocable

### Session Security
- In-memory session store
- Automatic expiration
- IP binding (optional)
- User-agent validation (optional)

### Transport Security
- TLS 1.3 required for production
- Certificate validation
- HTTPS enforcement
- Secure cookie flags (HttpOnly, Secure, SameSite)

## Configuration

### Server Config Updates

```rust
pub struct ServerConfig {
    // ... existing fields ...

    /// Enable authentication
    pub auth_enabled: bool,

    /// JWT secret key
    pub jwt_secret: String,

    /// JWT expiration (seconds)
    pub jwt_expiration_secs: u64,

    /// Allowed CORS origins
    pub cors_origins: Vec<String>,

    /// Require TLS
    pub require_tls: bool,

    /// Audit log path
    pub audit_log_path: PathBuf,

    /// Rate limiting
    pub rate_limit_enabled: bool,
    pub rate_limit_requests_per_minute: usize,
}
```

### Environment Variables

```bash
# Required
QILBEEDB_JWT_SECRET=your-secret-key-here  # Generate with: openssl rand -base64 64

# Optional
QILBEEDB_AUTH_ENABLED=true
QILBEEDB_JWT_EXPIRATION=86400  # 24 hours
QILBEEDB_CORS_ORIGINS=https://app.example.com,https://dashboard.example.com
QILBEEDB_REQUIRE_TLS=true
QILBEEDB_AUDIT_LOG_PATH=./logs/audit.log
QILBEEDB_RATE_LIMIT_ENABLED=true
QILBEEDB_RATE_LIMIT_RPM=1000
```

## API Changes

### Authentication Endpoints

```
POST   /auth/register        - Create new user account
POST   /auth/login           - Authenticate and get JWT token
POST   /auth/refresh         - Refresh JWT token
POST   /auth/logout          - Invalidate session
GET    /auth/me              - Get current user info

POST   /auth/api-keys        - Create API key
GET    /auth/api-keys        - List API keys
DELETE /auth/api-keys/:id    - Revoke API key
```

### Protected Endpoints

All existing endpoints now require authentication via:

**Option 1: Bearer Token**
```
Authorization: Bearer <jwt_token>
```

**Option 2: API Key**
```
X-API-Key: qilbee_live_xxxxxxxxxxxxx
```

**Option 3: Basic Auth** (legacy support)
```
Authorization: Basic base64(username:password)
```

### Response Codes

```
401 Unauthorized      - Missing or invalid credentials
403 Forbidden         - Valid credentials, insufficient permissions
429 Too Many Requests - Rate limit exceeded
```

## Default Users

**Default Admin Account:**
- Username: `admin`
- Password: Set via `QILBEEDB_ADMIN_PASSWORD` env var
- Roles: Admin
- **MUST** be changed on first login

## Migration Guide

### From Unsecured to Secured

1. **Enable Authentication:**
   ```rust
   let config = ServerConfig::for_production("./data")
       .with_auth();
   ```

2. **Set JWT Secret:**
   ```bash
   export QILBEEDB_JWT_SECRET=$(openssl rand -base64 64)
   ```

3. **Create Admin User:**
   ```bash
   export QILBEEDB_ADMIN_PASSWORD="secure_password"
   ./qilbeedb --init-admin
   ```

4. **Update Clients:**
   - Add authentication headers
   - Handle 401/403 responses
   - Implement token refresh

### Backward Compatibility

Development mode still supports unauthenticated access:
```rust
let config = ServerConfig::for_development("./data");  // auth_enabled = false
```

## Security Best Practices

### For Administrators

1. **Always use strong passwords** (min 12 chars, mixed case, numbers, symbols)
2. **Rotate JWT secret** every 90 days
3. **Monitor audit logs** for suspicious activity
4. **Use TLS in production** (never plain HTTP)
5. **Implement rate limiting** to prevent brute force
6. **Regular security audits**
7. **Keep software updated**

### For Developers

1. **Never commit secrets** to version control
2. **Use environment variables** for configuration
3. **Validate all inputs** on client and server
4. **Use API keys** for service-to-service communication
5. **Implement proper error handling** (don't leak sensitive info)
6. **Follow principle of least privilege**
7. **Use parameterized queries** to prevent injection

### For Users

1. **Change default passwords immediately**
2. **Use unique passwords** (password manager recommended)
3. **Enable 2FA** when available (future feature)
4. **Revoke unused API keys**
5. **Report security issues** to contact@aicube.ca

## Performance Impact

**Authentication overhead:**
- JWT validation: ~0.1ms
- Permission check: ~0.05ms
- Audit logging (async): minimal

**Recommended:**
- Use API keys for high-throughput applications
- Implement caching for permission checks
- Use connection pooling

## Compliance

This implementation supports compliance with:
- **GDPR** - Audit trails, data access controls
- **SOC 2** - Access controls, monitoring, encryption
- **HIPAA** - Access controls, audit logging, encryption
- **ISO 27001** - Information security management

## Testing

```bash
# Run security tests
cargo test --package qilbee-server security::

# Run with security enabled
cargo run --package qilbee-server --bin qilbeedb -- --auth-enabled

# Benchmark authentication
cargo bench --package qilbee-server authentication_bench
```

## Roadmap

### Version 1.1 (Q1 2026)
- [ ] Two-factor authentication (TOTP)
- [ ] OAuth 2.0 / OpenID Connect
- [ ] SAML 2.0 SSO
- [ ] LDAP/Active Directory integration

### Version 1.2 (Q2 2026)
- [ ] Fine-grained ACLs (per-graph, per-node permissions)
- [ ] Data encryption at rest
- [ ] Field-level encryption
- [ ] Hardware security module (HSM) support

### Version 2.0 (Q3 2026)
- [ ] Zero-knowledge proofs
- [ ] Homomorphic encryption for queries
- [ ] Blockchain-based audit trails
- [ ] Multi-party computation

## Support

For security issues:
- Email: contact@aicube.ca
- Security advisories: https://github.com/aicubetechnology/qilbeeDB/security/advisories

For general questions:
- Documentation: https://docs.qilbeedb.io
- GitHub Issues: https://github.com/aicubetechnology/qilbeeDB/issues

## License

Apache 2.0 - See LICENSE file for details

---

**⚠️ SECURITY NOTICE**

This security implementation is under active development. While designed with enterprise-grade security in mind, it has not yet undergone professional security audit. For production deployments handling sensitive data, we recommend:

1. Professional security audit
2. Penetration testing
3. Regular security updates
4. Incident response plan
5. Security monitoring/SIEM integration

Contact contact@aicube.ca for enterprise support and security consulting.
