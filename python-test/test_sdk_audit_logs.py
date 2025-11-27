#!/usr/bin/env python3
"""
Python SDK tests for QilbeeDB audit logging functionality.
Tests the audit log methods in the QilbeeDB client.
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


def test_get_audit_logs_requires_auth():
    """Test that audit logs require authentication."""
    print("Testing audit logs require authentication...")

    db = QilbeeDB(SERVER_URL)
    # Not logged in - should fail
    try:
        db.get_audit_logs()
        assert False, "Should have raised AuthenticationError"
    except AuthenticationError:
        pass  # Expected

    print("  PASS: Audit logs require authentication")


def test_get_audit_logs_basic():
    """Test basic audit log query via SDK."""
    print("Testing basic audit log query via SDK...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    result = db.get_audit_logs()

    assert "events" in result, "Response should contain 'events'"
    assert "count" in result, "Response should contain 'count'"
    assert "limit" in result, "Response should contain 'limit'"
    assert isinstance(result["events"], list), "Events should be a list"

    print(f"  Found {result['count']} audit events")
    print("  PASS: Basic audit log query works")


def test_get_audit_logs_with_event_type_filter():
    """Test audit log filtering by event type."""
    print("Testing audit log event type filter...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Filter by login events
    result = db.get_audit_logs(event_type="login")

    for event in result.get("events", []):
        assert event["event_type"] == "login", f"Unexpected event type: {event['event_type']}"

    print(f"  Found {result['count']} login events")
    print("  PASS: Event type filter works")


def test_get_audit_logs_with_result_filter():
    """Test audit log filtering by result."""
    print("Testing audit log result filter...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Filter by success
    result = db.get_audit_logs(result="success")

    for event in result.get("events", []):
        assert event["result"] == "success", f"Unexpected result: {event['result']}"

    print(f"  Found {result['count']} successful events")
    print("  PASS: Result filter works")


def test_get_audit_logs_with_limit():
    """Test audit log limit parameter."""
    print("Testing audit log limit parameter...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    result = db.get_audit_logs(limit=5)

    assert len(result.get("events", [])) <= 5, f"Expected at most 5 events, got {len(result['events'])}"
    assert result.get("limit") == 5, f"Expected limit 5, got {result.get('limit')}"

    print(f"  Returned {len(result['events'])} events with limit=5")
    print("  PASS: Limit parameter works")


def test_get_failed_logins():
    """Test convenience method for failed logins."""
    print("Testing get_failed_logins convenience method...")

    # First, create a failed login
    db_fail = QilbeeDB(SERVER_URL)
    try:
        db_fail.login(ADMIN_USERNAME, "wrong_password")
    except AuthenticationError:
        pass  # Expected

    time.sleep(0.5)  # Give server time to log

    # Now query using the admin account
    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    failed = db.get_failed_logins(limit=10)

    assert isinstance(failed, list), "Should return a list"
    assert len(failed) > 0, "Should have at least one failed login"

    for event in failed:
        assert event["event_type"] == "login_failed", f"Unexpected event type: {event['event_type']}"

    print(f"  Found {len(failed)} failed login events")
    print("  PASS: get_failed_logins works")


def test_get_user_audit_events():
    """Test convenience method for user-specific events."""
    print("Testing get_user_audit_events convenience method...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    events = db.get_user_audit_events(ADMIN_USERNAME, limit=20)

    assert isinstance(events, list), "Should return a list"

    for event in events:
        # Events should be related to admin user
        if event.get("username"):
            assert event["username"] == ADMIN_USERNAME, f"Unexpected username: {event['username']}"

    print(f"  Found {len(events)} events for user '{ADMIN_USERNAME}'")
    print("  PASS: get_user_audit_events works")


def test_get_security_events():
    """Test convenience method for security events."""
    print("Testing get_security_events convenience method...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    events = db.get_security_events(limit=20)

    assert isinstance(events, list), "Should return a list"

    for event in events:
        assert event["result"] in ["unauthorized", "forbidden"], \
            f"Unexpected result: {event['result']}"

    print(f"  Found {len(events)} security-relevant events")
    print("  PASS: get_security_events works")


def test_audit_logs_records_user_operations():
    """Test that user operations are recorded in audit logs."""
    print("Testing user operations create audit events...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Create a test user
    test_username = f"auditsdkuser_{int(time.time())}"
    user = db.create_user(
        username=test_username,
        password="TestPass123!@#",
        email=f"{test_username}@example.com"
    )
    user_id = user["id"]

    time.sleep(0.5)  # Give server time to log

    # Check for user_created event
    result = db.get_audit_logs(event_type="user_created", limit=10)
    found = any(
        test_username in str(event.get("metadata", {})) or
        test_username in str(event.get("resource", ""))
        for event in result.get("events", [])
    )
    assert found, f"No audit event found for creating user {test_username}"
    print(f"  Found audit event for user creation: {test_username}")

    # Delete the test user
    db.delete_user(user_id)

    time.sleep(0.5)  # Give server time to log

    # Check for user_deleted event
    result = db.get_audit_logs(event_type="user_deleted", limit=10)
    assert len(result.get("events", [])) > 0, "No user_deleted events found"
    print("  Found audit event for user deletion")

    print("  PASS: User operations create audit events")


def test_audit_logs_records_api_key_operations():
    """Test that API key operations are recorded in audit logs."""
    print("Testing API key operations create audit events...")

    db = QilbeeDB(SERVER_URL)
    db.login(ADMIN_USERNAME, ADMIN_PASSWORD)

    # Create an API key
    key = db.create_api_key(name="sdk_audit_test_key")
    key_id = key["id"]

    time.sleep(0.5)  # Give server time to log

    # Check for api_key_created event
    result = db.get_audit_logs(event_type="api_key_created", limit=10)
    assert len(result.get("events", [])) > 0, "No api_key_created events found"
    print("  Found audit event for API key creation")

    # Delete the API key
    db.delete_api_key(key_id)

    time.sleep(0.5)  # Give server time to log

    # Check for api_key_revoked event
    result = db.get_audit_logs(event_type="api_key_revoked", limit=10)
    assert len(result.get("events", [])) > 0, "No api_key_revoked events found"
    print("  Found audit event for API key revocation")

    print("  PASS: API key operations create audit events")


def main():
    """Run all SDK audit logging tests."""
    print("=" * 60)
    print("QilbeeDB Python SDK Audit Logging Tests")
    print("=" * 60)

    # Check if server is running
    if not wait_for_server(timeout=5):
        print("Server not running. Starting server...")
        data_dir = os.path.join(os.path.dirname(__file__), '..', 'test_audit_data')
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
            test_get_audit_logs_requires_auth,
            test_get_audit_logs_basic,
            test_get_audit_logs_with_event_type_filter,
            test_get_audit_logs_with_result_filter,
            test_get_audit_logs_with_limit,
            test_get_failed_logins,
            test_get_user_audit_events,
            test_get_security_events,
            test_audit_logs_records_user_operations,
            test_audit_logs_records_api_key_operations,
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
        if server_process:
            print("Stopping server...")
            server_process.terminate()
            server_process.wait(timeout=5)


if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)
