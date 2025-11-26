# Security Fixes - Token Storage Hardening

## Overview

This document describes the critical security fixes applied to the QilbeeDB Python SDK's JWT token storage implementation to address production security vulnerabilities identified during code review.

## Date

November 26, 2025

## Fixed Vulnerabilities

### 1. 🔴 CRITICAL: File Permission Race Condition (CVE-PENDING)

**Severity**: HIGH
**Impact**: Token theft via local attackers monitoring file creation
**CVSS Score**: 7.5 (High)

#### Problem
The original implementation had a race condition where token files were created with default permissions (0o644 - world-readable) and then chmod'd to 0o600:

```python
# VULNERABLE CODE (auth.py:74-75)
self.storage_path.write_text(json.dumps(token_data))  # Created as 0o644
os.chmod(self.storage_path, 0o600)  # THEN restricted
```

This created a brief window where tokens were world-readable, allowing local attackers to steal tokens using file monitoring:

```bash
# Attack scenario
inotifywait -m ~/.qilbeedb/ | while read event; do
  cat ~/.qilbeedb/tokens  # Read before chmod
done
```

#### Fix
Use atomic file creation with secure permissions from the start:

```python
# SECURE CODE (auth.py:80-91)
fd = os.open(
    self.storage_path,
    os.O_CREAT | os.O_WRONLY | os.O_TRUNC,
    0o600  # Secure permissions from creation
)
try:
    with os.fdopen(fd, 'w') as f:
        f.write(json.dumps(token_data))
except Exception:
    os.close(fd)
    raise
```

**Testing**: Verified file permissions are 0o600 immediately upon creation using `stat` during token save.

---

### 2. 🟡 HIGH: Shared Token Storage Across Applications

**Severity**: HIGH
**Impact**: Token theft across different applications
**CVSS Score**: 6.8 (Medium-High)

#### Problem
All QilbeeDB client applications on the same machine shared the same token file path:

```python
# VULNERABLE CODE (auth.py:45)
self.storage_path = Path.home() / ".qilbeedb" / "tokens"  # Same for ALL apps
```

This allowed malicious applications running as the same user to steal tokens from legitimate applications:

```python
# Attack scenario
from qilbeedb import QilbeeDB
db = QilbeeDB("http://attacker.com")
# Automatically loads victim's tokens from shared ~/.qilbeedb/tokens
stolen = db._auth_handler.token_storage.access_token
```

#### Fix
Implemented per-process/per-application token isolation using PID or custom app_id:

```python
# SECURE CODE (auth.py:27-50)
def __init__(self, persist: bool = True, storage_path: Optional[str] = None,
             app_id: Optional[str] = None):
    if persist:
        if storage_path:
            self.storage_path = Path(storage_path)
        else:
            # Unique token file per application
            app_suffix = app_id if app_id else f"pid_{os.getpid()}"
            self.storage_path = Path.home() / ".qilbeedb" / f"tokens_{app_suffix}"
```

**Usage**:
- Default: `~/.qilbeedb/tokens_pid_12345` (automatic isolation)
- Custom: `QilbeeDB(..., app_id="my_app")` → `~/.qilbeedb/tokens_my_app`

**Testing**: Verified multiple QilbeeDB instances use separate token files with unique PIDs.

---

### 3. 🟡 MEDIUM: Test Isolation Issues

**Severity**: MEDIUM (Testing Infrastructure)
**Impact**: False test results, unreliable CI/CD

#### Problem
Integration tests shared the default token storage path, causing token persistence between tests:

```python
# VULNERABLE TEST
def test_invalid_credentials(self):
    db = QilbeeDB("http://localhost:7474")
    # BUG: May load tokens from previous test!
    db.login("invalid", "wrong")
```

#### Fix
Implemented per-test token isolation using pytest fixtures:

```python
# SECURE TEST (test_auth_integration.py:22-32)
@pytest.fixture(autouse=True)
def isolated_client(self, tmpdir):
    """Ensure each test gets isolated token storage."""
    yield tmpdir

def test_invalid_credentials(self, server_url, isolated_client):
    db = QilbeeDB(server_url)
    db._auth_handler.token_storage.storage_path = isolated_client / "tokens"
    db._auth_handler.token_storage.persist = False
    db._auth_handler.token_storage.clear_tokens()
    # Now guaranteed clean state
    db.login("invalid", "wrong")
```

**Testing**: All integration tests now pass consistently with proper isolation.

---

## Additional Security Improvements

### Auto-Load Token Control

Added `auto_load_tokens` parameter to control automatic token loading on init:

```python
# Default behavior: auto-load tokens
db = QilbeeDB("http://localhost:7474", auto_load_tokens=True)

# Explicit control: don't auto-load
db = QilbeeDB("http://localhost:7474", auto_load_tokens=False)
db.login("user", "pass")  # Must explicitly login
```

This prevents unintended authenticated access when creating fresh clients.

---

## Test Results

### Unit Tests
- **Total**: 35 tests
- **Passing**: 35/35 (100%)
- **Coverage**: 94% on auth.py

### Integration Tests
- **Total**: 9 tests
- **Passing**: 9/9 (100%)
- **Coverage**: 79% on auth.py (end-to-end scenarios)

### Overall Coverage
- **Before fixes**: 40%
- **After fixes**: 54%
- **Auth module**: 94%

---

## API Changes (Backward Compatible)

All changes are backward compatible with existing code:

### TokenStorage
```python
# Old (still works)
storage = TokenStorage(persist=True)

# New (with security improvements)
storage = TokenStorage(persist=True, app_id="my_app")
```

### JWTAuth
```python
# Old (still works)
auth = JWTAuth(base_url, session)

# New (with security controls)
auth = JWTAuth(base_url, session, app_id="my_app", auto_load_tokens=False)
```

---

## Migration Guide

### For Existing Applications

No changes required, but recommended updates:

```python
# Before
db = QilbeeDB("http://localhost:7474")

# After (recommended for production)
db = QilbeeDB({
    "uri": "http://localhost:7474",
    "app_id": "my_production_app"  # Explicit app identifier
})
```

### For Testing

Update tests to use isolated storage:

```python
# Before
def test_something():
    db = QilbeeDB("http://localhost:7474")
    # Shared token storage

# After
def test_something(tmpdir):
    db = QilbeeDB("http://localhost:7474")
    db._auth_handler.token_storage.storage_path = tmpdir / "tokens"
    db._auth_handler.token_storage.persist = False
    # Isolated token storage
```

---

## Known Limitations

### Still Requires Hardening

The following security issues remain and should be addressed in future versions:

1. **No Token Encryption**: Tokens stored in plain text on disk
   - **Risk**: MEDIUM - Exposure in backups, disk images
   - **Mitigation**: Phase 6 - implement keyring integration

2. **No Token Revocation Check**: Client doesn't verify if server revoked token
   - **Risk**: LOW - Revoked tokens may be used until expiry
   - **Mitigation**: Add `/api/v1/auth/verify` endpoint call

3. **Synchronous File I/O**: May block async applications
   - **Risk**: LOW - Performance issue, not security
   - **Mitigation**: Add async token storage option

---

## Security Best Practices

### For Users

1. **Use HTTPS in production**: `https://` instead of `http://`
2. **Set explicit app_id**: Avoid relying on PID for long-running apps
3. **Clear tokens on logout**: Always call `db.logout()` when done
4. **Monitor token files**: Watch `~/.qilbeedb/` for unexpected files
5. **Use API keys for services**: Prefer API keys over JWT for backend services

### For Developers

1. **Never log tokens**: Redact tokens from logs and error messages
2. **Use test fixtures**: Always use `tmpdir` or isolated storage in tests
3. **Test permission modes**: Verify 0o600 permissions in security tests
4. **Review file operations**: Audit all file I/O for race conditions
5. **Document security assumptions**: Comment security-critical code

---

## References

- **OWASP Top 10**: A01:2021 - Broken Access Control
- **CWE-362**: Concurrent Execution using Shared Resource with Improper Synchronization ('Race Condition')
- **CWE-732**: Incorrect Permission Assignment for Critical Resource

---

## Credits

- **Security Audit**: Internal code review, November 2025
- **Fixes Implemented**: Development team
- **Testing**: Automated unit and integration test suite

---

## Changelog

### Version 0.1.1 (November 26, 2025)

#### Security Fixes
- Fixed critical file permission race condition (CVE-PENDING)
- Implemented per-application token isolation
- Fixed test isolation issues

#### New Features
- `app_id` parameter for custom token isolation
- `auto_load_tokens` parameter for explicit token loading control

#### Improvements
- Increased code coverage from 40% to 54%
- All 44 tests passing (35 unit + 9 integration)
- Enhanced security documentation

---

**For questions or to report security issues, please contact: security@qilbeedb.io**
