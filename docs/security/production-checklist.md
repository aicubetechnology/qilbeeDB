# Production Deployment Security Checklist

This checklist covers all security configurations required for deploying QilbeeDB in a production environment.

## Pre-Deployment Checklist

### 1. Authentication Configuration

- [ ] **Change default admin password**
  ```bash
  # After bootstrap, immediately change the admin password
  curl -X PUT "http://localhost:7474/api/v1/users/admin" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"password": "YourSecurePassword123!"}'
  ```

- [ ] **Password policy enforcement**
  - Minimum 12 characters
  - At least one uppercase letter
  - At least one lowercase letter
  - At least one digit
  - At least one special character (!@#$%^&*()_+-=[]{}|;:,.<>?)

- [ ] **JWT configuration**
  ```bash
  # Set secure JWT secret (minimum 32 characters)
  export JWT_SECRET="your-cryptographically-secure-secret-key-here"
  export JWT_EXPIRATION_SECS=3600  # 1 hour recommended
  ```

- [ ] **API key expiration**
  - Set appropriate expiration for API keys
  - Implement key rotation schedule (recommended: 90 days)

### 2. HTTPS Configuration

- [ ] **Enable HTTPS enforcement**
  ```bash
  export HTTPS_ENFORCE=true
  export HTTPS_PORT=443
  export HTTPS_ALLOW_LOCALHOST=false  # Disable in production
  export HTTPS_TRUST_PROXY=true       # If behind load balancer
  ```

- [ ] **TLS certificate configuration**
  ```bash
  export TLS_CERT_PATH=/path/to/certificate.pem
  export TLS_KEY_PATH=/path/to/private-key.pem
  export TLS_MIN_VERSION=1.2  # Minimum TLS 1.2, prefer 1.3
  ```

- [ ] **Certificate requirements**
  - Use certificates from trusted CA (not self-signed)
  - Ensure certificate covers all hostnames
  - Set up certificate renewal automation
  - Certificate expiry monitoring

### 3. CORS Configuration

- [ ] **Configure allowed origins**
  ```bash
  # Strict whitelist of allowed origins
  export CORS_ALLOWED_ORIGINS="https://app.yourdomain.com,https://admin.yourdomain.com"
  export CORS_ALLOW_CREDENTIALS=true
  export CORS_MAX_AGE=86400
  export CORS_PERMISSIVE=false  # NEVER use permissive mode in production
  ```

- [ ] **Verify CORS headers**
  ```bash
  curl -X OPTIONS "https://your-api.com/api/v1/health" \
    -H "Origin: https://app.yourdomain.com" \
    -H "Access-Control-Request-Method: POST" -v
  ```

### 4. Security Headers

- [ ] **Verify security headers are enabled**
  ```bash
  curl -sI "https://your-api.com/health" | grep -E "^(X-|Strict|Content-Security)"
  ```

- [ ] **Expected headers**
  | Header | Expected Value |
  |--------|----------------|
  | `Strict-Transport-Security` | `max-age=31536000; includeSubDomains` |
  | `X-Content-Type-Options` | `nosniff` |
  | `X-Frame-Options` | `DENY` |
  | `X-XSS-Protection` | `1; mode=block` |
  | `Content-Security-Policy` | Restrictive policy |
  | `Referrer-Policy` | `strict-origin-when-cross-origin` |
  | `Permissions-Policy` | Restrictive permissions |

### 5. Rate Limiting

- [ ] **Configure rate limits**
  ```bash
  # Defaults are reasonable, but customize as needed
  # Login: 100 req/min per IP
  # API key creation: 100 req/min per user
  # User management: 1000 req/min per user
  # General API: 100,000 req/min per user/key
  ```

- [ ] **Monitor rate limit events**
  ```bash
  curl "https://your-api.com/api/v1/audit-logs?event_type=rate_limit_exceeded" \
    -H "Authorization: Bearer $ADMIN_TOKEN"
  ```

### 6. Account Lockout

- [ ] **Configure lockout policy**
  - Default: 5 failed attempts triggers lockout
  - Progressive lockout duration (increases with each lockout)
  - Time-based automatic unlock

- [ ] **Monitor locked accounts**
  ```bash
  curl "https://your-api.com/api/v1/users/locked" \
    -H "Authorization: Bearer $ADMIN_TOKEN"
  ```

### 7. Audit Logging

- [ ] **Enable file persistence**
  ```bash
  # Configure audit log storage
  export AUDIT_LOG_PATH=/var/log/qilbeedb/audit
  export AUDIT_RETENTION_DAYS=365  # Based on compliance requirements
  export AUDIT_MAX_FILE_SIZE=52428800  # 50MB
  ```

- [ ] **Set up log rotation**
  - Automatic rotation by size
  - Archive to cold storage
  - Comply with retention requirements

- [ ] **Configure SIEM integration**
  - Forward logs to Elasticsearch/Splunk
  - Set up alerting rules
  - Configure dashboards

## Network Security

### 8. Firewall Rules

- [ ] **Restrict inbound traffic**
  ```bash
  # Allow only necessary ports
  # Port 7474: HTTP API
  # Port 7687: Bolt protocol
  # Port 443: HTTPS (if terminatingTLS)
  ```

- [ ] **Restrict outbound traffic**
  - Only allow necessary outbound connections
  - Block egress to untrusted networks

### 9. Load Balancer Configuration

- [ ] **Configure health checks**
  ```bash
  # Health endpoint
  GET /health
  # Ready endpoint
  GET /ready
  ```

- [ ] **Configure X-Forwarded headers**
  ```bash
  export HTTPS_TRUST_PROXY=true
  # Ensure load balancer sets:
  # - X-Forwarded-Proto
  # - X-Forwarded-For
  # - X-Real-IP
  ```

### 10. Network Isolation

- [ ] **Deploy in private subnet**
- [ ] **Use VPC/network policies**
- [ ] **No direct internet access to database**
- [ ] **Use bastion/jump host for admin access**

## Data Security

### 11. Data at Rest

- [ ] **Encrypt database files**
  - Use filesystem encryption (LUKS, BitLocker)
  - Or encrypted storage volumes

- [ ] **Secure backup encryption**
  ```bash
  # Encrypt backups
  tar czf - /data/qilbeedb | gpg --encrypt --recipient backup@company.com > backup.tar.gz.gpg
  ```

### 12. Data in Transit

- [ ] **TLS for all connections**
  - HTTP API: HTTPS only
  - Bolt protocol: TLS enabled
  - Internal communication: mTLS recommended

### 13. Secrets Management

- [ ] **Use secrets manager**
  - AWS Secrets Manager
  - HashiCorp Vault
  - Kubernetes Secrets (encrypted)

- [ ] **Never store secrets in**
  - Environment files (.env)
  - Container images
  - Git repositories
  - Configuration files

## Monitoring & Alerting

### 14. Security Monitoring

- [ ] **Set up alerts for**
  | Event | Threshold | Severity |
  |-------|-----------|----------|
  | Failed logins | > 10/5min from same IP | Critical |
  | Account lockouts | Any | High |
  | Permission denials | > 50/hour | Medium |
  | Rate limit violations | > 100/hour | Medium |
  | Admin role changes | Any | High |
  | API key creation | Any | Medium |

- [ ] **Monitor metrics**
  ```bash
  # Prometheus endpoint (if enabled)
  GET /metrics
  ```

### 15. Log Aggregation

- [ ] **Centralize logs**
  - Application logs
  - Audit logs
  - Access logs
  - Error logs

- [ ] **Retention policy**
  | Log Type | Retention |
  |----------|-----------|
  | Audit logs | Per compliance (1-7 years) |
  | Access logs | 90 days |
  | Error logs | 30 days |
  | Debug logs | 7 days |

## Operational Security

### 16. Access Control

- [ ] **Principle of least privilege**
  - Create users with minimum required roles
  - Use API keys for applications (not admin credentials)
  - Regular access reviews

- [ ] **Role assignments**
  | Role | Use Case |
  |------|----------|
  | Read | Read-only applications |
  | Write | Applications that modify data |
  | Admin | User management, configuration |
  | SuperAdmin | Full system access (limited users) |

### 17. Change Management

- [ ] **Document all changes**
- [ ] **Test in staging first**
- [ ] **Have rollback plan**
- [ ] **Audit trail for changes**

### 18. Incident Response

- [ ] **Incident response plan**
  - Contact information
  - Escalation procedures
  - Communication templates

- [ ] **Token revocation procedure**
  ```bash
  # Revoke all tokens for compromised user
  curl -X POST "https://your-api.com/api/v1/auth/revoke-all" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"user_id": "compromised_user_id"}'
  ```

- [ ] **API key rotation procedure**
  ```bash
  # Rotate compromised API key
  curl -X POST "https://your-api.com/api/v1/api-keys/rotate" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"key": "old_api_key"}'
  ```

## Compliance

### 19. Compliance Requirements

- [ ] **Identify applicable standards**
  - GDPR
  - HIPAA
  - SOC 2
  - PCI DSS
  - SOX

- [ ] **Configure audit retention**
  | Standard | Minimum Retention |
  |----------|-------------------|
  | GDPR | 6-12 months |
  | HIPAA | 6 years |
  | SOX | 7 years |
  | PCI DSS | 1 year |
  | SOC 2 | 1 year |

### 20. Regular Audits

- [ ] **Schedule security audits**
  - Weekly: Review failed authentications
  - Monthly: User access review
  - Quarterly: Full security audit
  - Annually: Penetration testing

## Quick Reference: Environment Variables

```bash
# Authentication
export JWT_SECRET="secure-random-string-min-32-chars"
export JWT_EXPIRATION_SECS=3600

# HTTPS
export HTTPS_ENFORCE=true
export HTTPS_PORT=443
export HTTPS_ALLOW_LOCALHOST=false
export HTTPS_TRUST_PROXY=true
export TLS_CERT_PATH=/path/to/cert.pem
export TLS_KEY_PATH=/path/to/key.pem
export TLS_MIN_VERSION=1.2

# CORS
export CORS_ALLOWED_ORIGINS="https://app.yourdomain.com"
export CORS_ALLOW_CREDENTIALS=true
export CORS_MAX_AGE=86400
export CORS_PERMISSIVE=false

# Audit
export AUDIT_LOG_PATH=/var/log/qilbeedb/audit
export AUDIT_RETENTION_DAYS=365
export AUDIT_MAX_FILE_SIZE=52428800
```

## Verification Script

Run this script to verify security configuration:

```bash
#!/bin/bash
# verify_security.sh

BASE_URL="https://your-api.com"
ERRORS=0

echo "QilbeeDB Production Security Verification"
echo "=========================================="

# Check HTTPS
echo -n "Checking HTTPS... "
if curl -sI "$BASE_URL/health" | grep -q "HTTP/2 200\|HTTP/1.1 200"; then
    echo "OK"
else
    echo "FAIL - HTTPS not working"
    ((ERRORS++))
fi

# Check security headers
echo -n "Checking security headers... "
HEADERS=$(curl -sI "$BASE_URL/health")
if echo "$HEADERS" | grep -q "Strict-Transport-Security"; then
    echo "OK"
else
    echo "FAIL - Missing HSTS header"
    ((ERRORS++))
fi

# Check X-Frame-Options
echo -n "Checking X-Frame-Options... "
if echo "$HEADERS" | grep -q "X-Frame-Options: DENY"; then
    echo "OK"
else
    echo "FAIL - Missing or incorrect X-Frame-Options"
    ((ERRORS++))
fi

# Check CORS
echo -n "Checking CORS configuration... "
CORS=$(curl -sI -X OPTIONS "$BASE_URL/api/v1/health" \
  -H "Origin: https://malicious-site.com" \
  -H "Access-Control-Request-Method: GET")
if echo "$CORS" | grep -q "Access-Control-Allow-Origin: https://malicious-site.com"; then
    echo "FAIL - CORS allows arbitrary origins"
    ((ERRORS++))
else
    echo "OK"
fi

# Check HTTP redirect
echo -n "Checking HTTP redirect... "
HTTP_RESPONSE=$(curl -sI "http://your-api.com/health" 2>/dev/null | head -1)
if echo "$HTTP_RESPONSE" | grep -q "301\|302"; then
    echo "OK"
else
    echo "WARN - HTTP not redirecting to HTTPS"
fi

echo ""
echo "Verification complete. Errors: $ERRORS"
exit $ERRORS
```

## Next Steps

- [Security Overview](overview.md) - Complete security documentation
- [Authentication](authentication.md) - Auth configuration details
- [Audit Logging](audit.md) - Audit log configuration
- [Audit Log Analysis](audit-analysis.md) - Security monitoring guide
- [Rate Limiting](rate-limiting.md) - Rate limit configuration
