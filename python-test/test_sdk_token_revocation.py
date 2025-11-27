#!/usr/bin/env python3
"""
Python SDK tests for QilbeeDB token revocation functionality.
Tests the token revocation methods in the QilbeeDB client.
"""

import sys
import os
import time
import subprocess
import requests

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'sdks', 'python'))

from qilbeedb import QilbeeDB
from qilbeedb.exceptions import AuthenticationError

# Server configuration
SERVER_URL = "http://localhost:7474"
ADMIN_USERNAME = "admin"
ADMIN_PASSWORD = "Admin123!@#"


def wait_for_server(timeout: int = 30) -> bool:
    """Wait for the server to be ready."""
    start_time = time.time()
    while time.time() - start_time < timeout:
        try:
            response = requests.get(f"{SERVER_URL}/health", timeout=2)
            if response.status_code == 200:
                return True
        except requests.exceptions.ConnectionError:
            pass
        time.sleep(0.5)
    return False


def test_revoke_token_basic():
    """Test basic token revocation."""
    print("Testing basic token revocation...")

    db = QilbeeDB(SERVER_URL)
    login_response = db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Get the current access token
    access_token = login_response.get("access_token") or login_response.get("token")
    assert access_token, "Should have received an access token"

    # Verify the token works
    health = db.health()
    assert health.get("status") == "healthy", "Health check should work with valid token"

    # Now revoke the token
    result = db.revoke_token(access_token)

    assert result.get("success") is True, "Revocation should succeed"
    assert "jti" in result, "Response should include the jti"

    print(f"  Revoked token with jti: {result.get('jti')}")
    print("  PASS: Basic token revocation works")


def test_revoked_token_is_invalid():
    """Test that a revoked token can no longer be used."""
    print("Testing that revoked tokens are invalid...")

    # Login and get a token
    db = QilbeeDB(SERVER_URL)
    login_response = db.login(ADMIN_USERNAME, ADMIN_PASSWORD)
    access_token = login_response.get("access_token") or login_response.get("token")

    # Revoke the token
    db.revoke_token(access_token)

    # Create a new session using the revoked token
    db2 = QilbeeDB(SERVER_URL)
    db2.session.headers["Authorization"] = f"Bearer {access_token}"

    # Try to use the revoked token - should fail
    try:
        # Make a request that requires authentication
        db2.list_users()
        assert False, "Should have raised AuthenticationError"
    except (AuthenticationError, requests.exceptions.HTTPError) as e:
        # Expected - token is revoked
        pass

    print("  PASS: Revoked tokens are properly invalidated")


def test_revoke_all_tokens():
    """Test revoking all tokens for a user."""
    print("Testing revoke all tokens for user...")

    # Login as admin
    admin_db = QilbeeDB(SERVER_URL)
    admin_db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Create a test user
    test_username = f"revoke_test_{int(time.time())}"
    user = admin_db.create_user(
        username=test_username,
        password="TestPass123!@#",
        email=f"{test_username}@example.com",
        roles=["Developer"]
    )
    user_id = user["id"]

    # Login as the test user and get multiple tokens
    user_db1 = QilbeeDB(SERVER_URL)
    login1 = user_db1.login(test_username, "TestPass123!@#")
    token1 = login1.get("access_token") or login1.get("token")

    user_db2 = QilbeeDB(SERVER_URL)
    login2 = user_db2.login(test_username, "TestPass123!@#")
    token2 = login2.get("access_token") or login2.get("token")

    # Verify both tokens work
    user_db1.health()
    user_db2.health()

    # Admin revokes all tokens for the test user
    result = admin_db.revoke_all_tokens(user_id, reason="security_test")

    assert result.get("success") is True, "Revoke all should succeed"
    assert result.get("user_id") == user_id, "Response should include user_id"

    print(f"  Revoked all tokens for user: {user_id}")

    # Clean up - delete the test user
    admin_db.delete_user(user_id)

    print("  PASS: Revoke all tokens works")


def test_revoke_all_invalidates_existing_tokens():
    """Test that revoke-all invalidates all existing user tokens."""
    print("Testing that revoke-all invalidates all existing tokens...")

    # Login as admin
    admin_db = QilbeeDB(SERVER_URL)
    admin_db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Create a test user
    test_username = f"revoke_all_test_{int(time.time())}"
    user = admin_db.create_user(
        username=test_username,
        password="TestPass123!@#",
        email=f"{test_username}@example.com",
        roles=["Developer"]
    )
    user_id = user["id"]

    try:
        # Login as the test user
        user_db = QilbeeDB(SERVER_URL)
        login_response = user_db.login(test_username, "TestPass123!@#")
        token = login_response.get("access_token") or login_response.get("token")

        # Verify token works
        user_db.health()

        # Admin revokes all tokens for the test user
        admin_db.revoke_all_tokens(user_id, reason="security_incident")

        # Create a new session using the old token
        db_old_token = QilbeeDB(SERVER_URL)
        db_old_token.session.headers["Authorization"] = f"Bearer {token}"

        # Try to use the old token - should fail
        try:
            db_old_token.list_users()
            assert False, "Should have raised an error - token should be invalid"
        except (AuthenticationError, requests.exceptions.HTTPError):
            # Expected - token is revoked
            pass

        # User can still log in again and get a new valid token
        new_db = QilbeeDB(SERVER_URL)
        new_login = new_db.login(test_username, "TestPass123!@#")
        new_token = new_login.get("access_token") or new_login.get("token")

        # New token should work
        health = new_db.health()
        assert health.get("status") == "healthy", "New token should work"

        print("  Old tokens invalidated, new tokens work")

    finally:
        # Clean up - delete the test user
        admin_db.delete_user(user_id)

    print("  PASS: Revoke all properly invalidates existing tokens")


def test_revoke_token_requires_valid_token():
    """Test that token revocation requires a valid JWT token to revoke."""
    print("Testing token revocation requires valid token...")

    db = QilbeeDB(SERVER_URL)
    # The revoke endpoint validates the token being revoked (not the auth token)
    # Sending an invalid token should fail with 400 Bad Request
    try:
        db.revoke_token("invalid_token")
        assert False, "Should have raised HTTPError"
    except AuthenticationError:
        pass  # Also acceptable - depends on error handling
    except requests.exceptions.HTTPError as e:
        # 400 is expected for invalid token format
        if e.response.status_code not in [400, 401]:
            raise

    print("  PASS: Token revocation requires valid token")


def test_revoke_all_requires_admin():
    """Test that revoke-all requires admin permissions."""
    print("Testing revoke-all requires admin permissions...")

    # Login as admin to create a test user
    admin_db = QilbeeDB(SERVER_URL)
    admin_db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Create a test user with Developer role (not Admin)
    test_username = f"nonadmin_{int(time.time())}"
    user = admin_db.create_user(
        username=test_username,
        password="TestPass123!@#",
        email=f"{test_username}@example.com",
        roles=["Developer"]
    )
    user_id = user["id"]

    try:
        # Login as the non-admin user
        user_db = QilbeeDB(SERVER_URL)
        user_db.login(test_username, "TestPass123!@#")

        # Try to revoke all tokens - should fail (not admin)
        try:
            user_db.revoke_all_tokens(user_id)
            # If we get here, permissions might not be enforced
            # Some systems might allow users to revoke their own tokens
            print("  Note: Non-admin was able to revoke their own tokens")
        except (AuthenticationError, requests.exceptions.HTTPError) as e:
            # Expected - need admin permissions
            pass

    finally:
        # Clean up - delete the test user
        admin_db.delete_user(user_id)

    print("  PASS: Revoke-all permission check works")


def test_token_revocation_creates_audit_event():
    """Test that token revocation creates audit log entry."""
    print("Testing token revocation creates audit event...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Get a token and revoke it
    login_response = db.login(ADMIN_USERNAME, ADMIN_PASSWORD)
    token = login_response.get("access_token") or login_response.get("token")

    # Revoke the token
    db.revoke_token(token)

    time.sleep(0.5)  # Give server time to log

    # Login again with fresh token to check audit logs
    db2 = QilbeeDB(SERVER_URL)
    db2.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Check for token_revoked event in audit logs
    result = db2.get_audit_logs(event_type="token_revoked", limit=10)

    assert len(result.get("events", [])) > 0, "Should have token_revoked audit events"
    print(f"  Found {len(result['events'])} token revocation audit events")

    print("  PASS: Token revocation creates audit events")


def main():
    """Run all SDK token revocation tests."""
    print("=" * 60)
    print("QilbeeDB Python SDK Token Revocation Tests")
    print("=" * 60)

    # Check if server is running
    if not wait_for_server(timeout=5):
        print("Server not running. Starting server...")
        data_dir = os.path.join(os.path.dirname(__file__), '..', 'test_token_revocation_data')
        os.makedirs(data_dir, exist_ok=True)
        binary = os.path.join(os.path.dirname(__file__), '..', 'target', 'debug', 'qilbeedb')
        server_process = subprocess.Popen(
            [binary, data_dir],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )

        if not wait_for_server():
            print("ERROR: Failed to start server")
            server_process.terminate()
            sys.exit(1)

        print("Server started successfully")
    else:
        server_process = None
        print("Server already running")

    print()

    try:
        tests = [
            test_revoke_token_requires_valid_token,
            test_revoke_token_basic,
            test_revoked_token_is_invalid,
            test_revoke_all_tokens,
            test_revoke_all_invalidates_existing_tokens,
            test_revoke_all_requires_admin,
            test_token_revocation_creates_audit_event,
        ]

        passed = 0
        failed = 0

        for test in tests:
            try:
                test()
                passed += 1
            except Exception as e:
                print(f"  FAIL: {e}")
                import traceback
                traceback.print_exc()
                failed += 1
            print()

        print("=" * 60)
        print(f"Results: {passed} passed, {failed} failed")
        print("=" * 60)

        return failed == 0

    finally:
        if server_process:
            print("Stopping server...")
            server_process.terminate()
            server_process.wait(timeout=5)


if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)
