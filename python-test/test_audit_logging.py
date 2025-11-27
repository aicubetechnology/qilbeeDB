#!/usr/bin/env python3
"""
Integration tests for QilbeeDB audit logging functionality.
Tests the audit log API endpoint and event logging for various operations.
"""

import os
import time
import subprocess
import requests
import signal
import sys

# Server configuration
SERVER_URL = "http://localhost:7474"
AUTH_URL = f"{SERVER_URL}/api/v1/auth"
USERS_URL = f"{SERVER_URL}/api/v1/users"
API_KEYS_URL = f"{SERVER_URL}/api/v1/api-keys"
AUDIT_LOGS_URL = f"{SERVER_URL}/api/v1/audit-logs"

# Test credentials
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


def login(username: str = ADMIN_USERNAME, password: str = ADMIN_PASSWORD) -> str:
    """Login and return access token."""
    response = requests.post(
        f"{AUTH_URL}/login",
        json={"username": username, "password": password}
    )
    if response.status_code != 200:
        raise Exception(f"Login failed: {response.status_code} {response.text}")
    return response.json()["access_token"]


def get_auth_headers(token: str) -> dict:
    """Get authorization headers for API requests."""
    return {"Authorization": f"Bearer {token}"}


def test_audit_logs_requires_admin():
    """Test that audit logs endpoint requires admin authentication."""
    print("Testing audit logs requires admin auth...")

    # Try without auth - should fail
    response = requests.get(AUDIT_LOGS_URL)
    assert response.status_code == 401, f"Expected 401, got {response.status_code}"

    print("  PASS: Audit logs requires authentication")


def test_audit_logs_query():
    """Test basic audit log query."""
    print("Testing audit log query...")

    token = login()
    headers = get_auth_headers(token)

    # Query all audit logs
    response = requests.get(AUDIT_LOGS_URL, headers=headers)
    assert response.status_code == 200, f"Expected 200, got {response.status_code}: {response.text}"

    data = response.json()
    assert "events" in data, "Response should contain 'events'"
    assert "count" in data, "Response should contain 'count'"
    assert "limit" in data, "Response should contain 'limit'"

    print(f"  Found {data['count']} audit events")
    print("  PASS: Audit log query works")


def test_audit_log_filtering():
    """Test audit log filtering by event type."""
    print("Testing audit log filtering...")

    token = login()
    headers = get_auth_headers(token)

    # Filter by event type
    response = requests.get(
        f"{AUDIT_LOGS_URL}?event_type=token_refresh",
        headers=headers
    )
    assert response.status_code == 200, f"Expected 200, got {response.status_code}"

    data = response.json()
    for event in data.get("events", []):
        assert event["event_type"] == "token_refresh", f"Unexpected event type: {event['event_type']}"

    print(f"  Found {data['count']} token_refresh events")
    print("  PASS: Audit log filtering works")


def test_user_management_audit_events():
    """Test that user management operations create audit events."""
    print("Testing user management audit events...")

    token = login()
    headers = get_auth_headers(token)

    # Create a test user
    test_username = f"audituser_{int(time.time())}"
    response = requests.post(
        USERS_URL,
        headers=headers,
        json={
            "username": test_username,
            "email": f"{test_username}@example.com",
            "password": "TestPass123!@#"
        }
    )
    assert response.status_code == 201, f"User creation failed: {response.status_code} {response.text}"
    user_id = response.json()["id"]

    # Give server time to log the event
    time.sleep(0.5)

    # Query for user_created events
    response = requests.get(
        f"{AUDIT_LOGS_URL}?event_type=user_created",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    # Find the event for our user
    found = False
    for event in data.get("events", []):
        if test_username in str(event.get("metadata", {})):
            found = True
            break

    assert found, f"No audit event found for creating user {test_username}"
    print(f"  Found audit event for user creation: {test_username}")

    # Delete the test user
    response = requests.delete(
        f"{USERS_URL}/{user_id}",
        headers=headers
    )
    assert response.status_code == 200, f"User deletion failed: {response.status_code}"

    # Give server time to log the event
    time.sleep(0.5)

    # Query for user_deleted events
    response = requests.get(
        f"{AUDIT_LOGS_URL}?event_type=user_deleted",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    assert len(data.get("events", [])) > 0, "No user_deleted events found"
    print("  Found audit event for user deletion")

    print("  PASS: User management creates audit events")


def test_api_key_audit_events():
    """Test that API key operations create audit events."""
    print("Testing API key audit events...")

    token = login()
    headers = get_auth_headers(token)

    # Create an API key
    response = requests.post(
        API_KEYS_URL,
        headers=headers,
        json={"name": "audit_test_key"}
    )
    assert response.status_code == 201, f"API key creation failed: {response.status_code}"
    key_id = response.json()["id"]

    # Give server time to log the event
    time.sleep(0.5)

    # Query for api_key_created events
    response = requests.get(
        f"{AUDIT_LOGS_URL}?event_type=api_key_created",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    assert len(data.get("events", [])) > 0, "No api_key_created events found"
    print("  Found audit event for API key creation")

    # Revoke the API key
    response = requests.delete(
        f"{API_KEYS_URL}/{key_id}",
        headers=headers
    )
    assert response.status_code == 200, f"API key revocation failed: {response.status_code}"

    # Give server time to log the event
    time.sleep(0.5)

    # Query for api_key_revoked events
    response = requests.get(
        f"{AUDIT_LOGS_URL}?event_type=api_key_revoked",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    assert len(data.get("events", [])) > 0, "No api_key_revoked events found"
    print("  Found audit event for API key revocation")

    print("  PASS: API key operations create audit events")


def test_failed_login_audit():
    """Test that failed login attempts create audit events."""
    print("Testing failed login audit...")

    # Attempt a failed login
    response = requests.post(
        f"{AUTH_URL}/login",
        json={"username": "admin", "password": "wrong_password"}
    )
    assert response.status_code == 401, f"Expected 401, got {response.status_code}"

    # Give server time to log the event
    time.sleep(0.5)

    # Login as admin and query for login_failed events
    token = login()
    headers = get_auth_headers(token)

    response = requests.get(
        f"{AUDIT_LOGS_URL}?event_type=login_failed",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    # Should have at least one failed login event
    assert len(data.get("events", [])) > 0, "No login_failed events found"
    print(f"  Found {data['count']} failed login events")

    print("  PASS: Failed login creates audit event")


def test_audit_log_limit():
    """Test audit log limit parameter."""
    print("Testing audit log limit parameter...")

    token = login()
    headers = get_auth_headers(token)

    # Query with limit=5
    response = requests.get(
        f"{AUDIT_LOGS_URL}?limit=5",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    assert len(data.get("events", [])) <= 5, f"Expected at most 5 events, got {len(data['events'])}"
    assert data.get("limit") == 5, f"Expected limit 5, got {data.get('limit')}"

    print(f"  Returned {len(data['events'])} events with limit=5")
    print("  PASS: Audit log limit works")


def test_audit_log_result_filter():
    """Test filtering audit logs by result."""
    print("Testing audit log result filter...")

    token = login()
    headers = get_auth_headers(token)

    # Filter by success
    response = requests.get(
        f"{AUDIT_LOGS_URL}?result=success",
        headers=headers
    )
    assert response.status_code == 200
    data = response.json()

    for event in data.get("events", []):
        assert event["result"] == "success", f"Unexpected result: {event['result']}"

    print(f"  Found {data['count']} successful events")
    print("  PASS: Audit log result filter works")


def main():
    """Run all audit logging tests."""
    print("=" * 60)
    print("QilbeeDB Audit Logging Integration Tests")
    print("=" * 60)

    # Check if server is running
    if not wait_for_server(timeout=5):
        print("Server not running. Starting server...")
        # Start server in background
        data_dir = os.path.join(os.path.dirname(__file__), '..', 'test_audit_data')
        binary = os.path.join(os.path.dirname(__file__), '..', 'target', 'debug', 'qilbeedb')
        os.makedirs(data_dir, exist_ok=True)
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
        # Run tests
        tests = [
            test_audit_logs_requires_admin,
            test_audit_logs_query,
            test_audit_log_filtering,
            test_failed_login_audit,
            test_user_management_audit_events,
            test_api_key_audit_events,
            test_audit_log_limit,
            test_audit_log_result_filter,
        ]

        passed = 0
        failed = 0

        for test in tests:
            try:
                test()
                passed += 1
            except Exception as e:
                print(f"  FAIL: {e}")
                failed += 1
            print()

        print("=" * 60)
        print(f"Results: {passed} passed, {failed} failed")
        print("=" * 60)

        return failed == 0

    finally:
        # Stop server if we started it
        if server_process:
            print("Stopping server...")
            server_process.terminate()
            server_process.wait(timeout=5)


if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)
