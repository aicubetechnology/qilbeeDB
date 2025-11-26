# QilbeeDB Security Bootstrap Guide

## Overview

QilbeeDB includes enterprise-grade security with an intelligent bootstrap system that automatically handles initial admin user creation on first deployment. This guide explains how the bootstrap process works and how to configure it for different deployment scenarios.

## How Bootstrap Works

When you deploy QilbeeDB with authentication enabled for the first time, the bootstrap process automatically runs to create an initial administrator account. The system is smart enough to detect whether you're running in an interactive environment (like a terminal) or non-interactive environment (like Docker or systemd).

### Bootstrap Detection

The bootstrap system checks for:
1. **Existing bootstrap state** - Looks for `.qilbee_bootstrap` file in the data directory
2. **TTY detection** - Determines if the session is interactive or non-interactive
3. **Environment variables** - Checks for admin credentials in environment variables

## Deployment Scenarios

### 1. Interactive Deployment (Development/Manual Setup)

When running QilbeeDB in an interactive terminal, you'll be prompted to create an admin account:

```bash
# Enable authentication in production mode
cargo run --bin qilbeedb --release
```

You'll see:

```
╔════════════════════════════════════════════════════════════╗
║     QilbeeDB First-Time Setup - Initial Admin Account     ║
╚════════════════════════════════════════════════════════════╝

This appears to be a fresh QilbeeDB installation.
Let's create your initial administrator account.

Enter admin username (or press Enter for 'admin'): admin
Enter admin email: admin@yourcompany.com

Password Requirements:
- Minimum 12 characters
- At least one uppercase letter
- At least one lowercase letter
- At least one number
- At least one special character (!@#$%^&*()_+-=[]{}|;:,.<>?)

Enter admin password: ************
Confirm admin password: ************

✅ Admin account created successfully!
   Username: admin
   Role: Administrator
```

### 2. Non-Interactive Deployment (Docker/Kubernetes/Systemd)

For automated deployments, use environment variables:

#### Docker

```dockerfile
FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin qilbeedb

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/qilbeedb /usr/local/bin/

# Bootstrap environment variables
ENV QILBEEDB_ADMIN_USERNAME=admin
ENV QILBEEDB_ADMIN_EMAIL=admin@example.com
ENV QILBEEDB_ADMIN_PASSWORD=YourSecurePassword123!

EXPOSE 7474 7687
CMD ["qilbeedb"]
```

#### Docker Compose

```yaml
version: '3.8'
services:
  qilbeedb:
    image: qilbeedb:latest
    environment:
      - QILBEEDB_ADMIN_USERNAME=admin
      - QILBEEDB_ADMIN_EMAIL=admin@example.com
      - QILBEEDB_ADMIN_PASSWORD=${ADMIN_PASSWORD}  # Use secrets
    ports:
      - "7474:7474"
      - "7687:7687"
    volumes:
      - qilbeedb-data:/data
    secrets:
      - admin_password

volumes:
  qilbeedb-data:

secrets:
  admin_password:
    file: ./secrets/admin_password.txt
```

#### Kubernetes

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: qilbeedb-admin
type: Opaque
stringData:
  username: admin
  email: admin@example.com
  password: YourSecurePassword123!  # Use sealed secrets in production

---
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
        image: qilbeedb:latest
        env:
        - name: QILBEEDB_ADMIN_USERNAME
          valueFrom:
            secretKeyRef:
              name: qilbeedb-admin
              key: username
        - name: QILBEEDB_ADMIN_EMAIL
          valueFrom:
            secretKeyRef:
              name: qilbeedb-admin
              key: email
        - name: QILBEEDB_ADMIN_PASSWORD
          valueFrom:
            secretKeyRef:
              name: qilbeedb-admin
              key: password
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

#### Systemd Service

```ini
[Unit]
Description=QilbeeDB Graph Database
After=network.target

[Service]
Type=simple
User=qilbeedb
Group=qilbeedb
WorkingDirectory=/opt/qilbeedb
ExecStart=/usr/local/bin/qilbeedb /var/lib/qilbeedb/data
Restart=always
RestartSec=10

# Bootstrap environment variables
Environment="QILBEEDB_ADMIN_USERNAME=admin"
Environment="QILBEEDB_ADMIN_EMAIL=admin@example.com"
EnvironmentFile=/etc/qilbeedb/admin.env

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/qilbeedb

[Install]
WantedBy=multi-user.target
```

`/etc/qilbeedb/admin.env`:
```bash
QILBEEDB_ADMIN_PASSWORD=YourSecurePassword123!
```

## Environment Variables

### Required Variables (Non-Interactive Mode)

| Variable | Description | Example |
|----------|-------------|---------|
| `QILBEEDB_ADMIN_EMAIL` | Admin email address (required) | `admin@example.com` |
| `QILBEEDB_ADMIN_PASSWORD` | Admin password (required) | `SecurePass123!` |

### Optional Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `QILBEEDB_ADMIN_USERNAME` | Admin username | `admin` |

## Password Requirements

The bootstrap system enforces strong password requirements to ensure security:

- **Minimum length**: 12 characters
- **Uppercase letter**: At least one (A-Z)
- **Lowercase letter**: At least one (a-z)
- **Number**: At least one (0-9)
- **Special character**: At least one (!@#$%^&*()_+-=[]{}|;:,.<>?)

### Valid Password Examples:

- ✅ `MySecureP@ssw0rd`
- ✅ `Adm!n2024Password`
- ✅ `C0mplex!tyRul3s`

### Invalid Password Examples:

- ❌ `shortpass` - Too short, no uppercase, no digit, no special char
- ❌ `ALLUPPERCASE123!` - No lowercase letter
- ❌ `alllowercase123!` - No uppercase letter
- ❌ `NoDigitsHere!` - No digit
- ❌ `NoSpecialChar123` - No special character

## Bootstrap State Tracking

After successful bootstrap, the system creates a `.qilbee_bootstrap` file in the data directory:

```json
{
  "is_bootstrapped": true,
  "admin_username": "admin",
  "bootstrapped_at": "2024-01-15T10:30:00Z"
}
```

This file:
- Prevents bootstrap from running again
- Records the admin username created
- Stores the timestamp of initial setup
- Should be backed up with your data directory

## Security Best Practices

### 1. Never Commit Credentials

```bash
# Add to .gitignore
.env
admin.env
secrets/
*.secret
```

### 2. Use Secret Management

**AWS Secrets Manager**:
```bash
# Store secret
aws secretsmanager create-secret \
  --name qilbeedb/admin-password \
  --secret-string "YourSecurePassword123!"

# Retrieve in startup script
export QILBEEDB_ADMIN_PASSWORD=$(aws secretsmanager get-secret-value \
  --secret-id qilbeedb/admin-password \
  --query SecretString \
  --output text)
```

**HashiCorp Vault**:
```bash
# Store secret
vault kv put secret/qilbeedb admin_password="YourSecurePassword123!"

# Retrieve in startup script
export QILBEEDB_ADMIN_PASSWORD=$(vault kv get -field=admin_password secret/qilbeedb)
```

**Kubernetes Sealed Secrets**:
```bash
# Create sealed secret
echo -n "YourSecurePassword123!" | kubectl create secret generic qilbeedb-admin \
  --dry-run=client --from-file=password=/dev/stdin -o yaml | \
  kubeseal -o yaml > sealed-secret.yaml
```

### 3. Rotate Initial Password

After initial deployment, immediately:

1. Log in with the bootstrap credentials
2. Change the admin password via the API
3. Optionally create additional admin users
4. Remove or rotate the environment variables

### 4. Audit Bootstrap Events

The bootstrap process is logged in the audit system:

```cypher
// Query bootstrap events
MATCH (event:AuditEvent)
WHERE event.action = 'create_user'
  AND event.username = 'admin'
RETURN event
ORDER BY event.timestamp DESC
LIMIT 1
```

## Troubleshooting

### Bootstrap Not Running

**Problem**: Server starts but bootstrap doesn't run

**Solutions**:
1. Check if authentication is enabled in config
2. Verify `.qilbee_bootstrap` file doesn't exist
3. Check server logs for bootstrap messages

### Password Validation Errors

**Problem**: Password rejected during bootstrap

**Solution**: Ensure password meets all requirements:
```bash
# Test password complexity
python3 << EOF
import re
password = "YourPasswordHere"
checks = [
    (len(password) >= 12, "At least 12 characters"),
    (re.search(r'[A-Z]', password), "Uppercase letter"),
    (re.search(r'[a-z]', password), "Lowercase letter"),
    (re.search(r'\d', password), "Number"),
    (re.search(r'[!@#\$%^&*()\-_+=\[\]{}|;:,.<>?]', password), "Special character"),
]
for check, desc in checks:
    print(f"{'✓' if check else '✗'} {desc}")
EOF
```

### Environment Variable Not Found

**Problem**: `QILBEEDB_ADMIN_EMAIL` or `QILBEEDB_ADMIN_PASSWORD` not set

**Solution**:
```bash
# Verify environment variables are set
echo $QILBEEDB_ADMIN_EMAIL
echo $QILBEEDB_ADMIN_PASSWORD

# Check they're available to the service
systemctl show qilbeedb --property=Environment
```

### Re-running Bootstrap

**Problem**: Need to re-run bootstrap after deletion

**Solution**:
```bash
# Remove bootstrap state file
rm /var/lib/qilbeedb/data/.qilbee_bootstrap

# Restart server - bootstrap will run again
systemctl restart qilbeedb
```

## Configuration Examples

### Development Environment

```bash
# Quick start for development (interactive)
cargo run --bin qilbeedb -- ./dev-data
```

### Production Environment

```bash
# Production deployment with environment variables
export QILBEEDB_ADMIN_USERNAME=admin
export QILBEEDB_ADMIN_EMAIL=admin@production.com
export QILBEEDB_ADMIN_PASSWORD=$(cat /run/secrets/admin_password)

./qilbeedb /var/lib/qilbeedb/data
```

### Cloud-Native Deployment

```yaml
# AWS ECS Task Definition
{
  "family": "qilbeedb",
  "containerDefinitions": [{
    "name": "qilbeedb",
    "image": "qilbeedb:latest",
    "secrets": [
      {
        "name": "QILBEEDB_ADMIN_PASSWORD",
        "valueFrom": "arn:aws:secretsmanager:region:account:secret:qilbeedb-admin"
      }
    ],
    "environment": [
      {"name": "QILBEEDB_ADMIN_USERNAME", "value": "admin"},
      {"name": "QILBEEDB_ADMIN_EMAIL", "value": "admin@example.com"}
    ]
  }]
}
```

## Summary

The QilbeeDB bootstrap system provides:

- ✅ **Automatic initial admin creation** on first deployment
- ✅ **Interactive and non-interactive modes** for all deployment scenarios
- ✅ **Strong password enforcement** with comprehensive validation
- ✅ **Environment variable support** for automated deployments
- ✅ **Bootstrap state tracking** to prevent re-initialization
- ✅ **Audit logging** of all security events
- ✅ **Docker, Kubernetes, and systemd ready** out of the box

For additional security features, see the main [SECURITY.md](./SECURITY.md) documentation.
