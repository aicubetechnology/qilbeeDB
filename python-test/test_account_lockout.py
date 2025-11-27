#!/usr/bin/env python3
"""
Test script for Account Lockout feature.

Tests:
1. Failed login tracking
2. Account lockout after N failed attempts
3. Admin lockout management (lock, unlock, get status)
4. Python SDK lockout methods
"""

import sys
import time
sys.path.insert(0, '../sdks/python')

from qilbeedb import QilbeeDB
import requests

BASE_URL = "http://localhost:7474"
ADMIN_USER = "admin"
ADMIN_PASS = "Admin123!@#"

# Create test user credentials
TEST_USER = "lockout_test_user"
TEST_PASS = "TestPass123!@#"

def print_test(name, passed):
    """Print test result."""
    status = "PASS" if passed else "FAIL"
    symbol = "[+]" if passed else "[-]"
    print(f"{symbol} {name}: {status}")
    return passed

def create_test_user(db):
    """Create a test user for lockout testing."""
    try:
        db.create_user(TEST_USER, TEST_PASS, roles=["Read"])
        print(f"Created test user: {TEST_USER}")
        return True
    except Exception as e:
        if "already exists" in str(e).lower():
            print(f"Test user {TEST_USER} already exists")
            return True
        print(f"Failed to create test user: {e}")
        return False

def delete_test_user(db):
    """Delete the test user."""
    try:
        db.delete_user(TEST_USER)
        print(f"Deleted test user: {TEST_USER}")
    except Exception:
        pass

def test_failed_login_tracking():
    """Test that failed logins are tracked in response."""
    print("\n=== Test 1: Failed Login Tracking ===")

    response = requests.post(
        f"{BASE_URL}/api/v1/auth/login",
        json={"username": "nonexistent_user", "password": "wrong_pass"}
    )

    result = response.json()

    # Check that failed_attempts and remaining_attempts are in response
    has_tracking = "failed_attempts" in result and "remaining_attempts" in result

    if has_tracking:
        print(f"  Failed attempts: {result.get('failed_attempts')}")
        print(f"  Remaining attempts: {result.get('remaining_attempts')}")

    return print_test("Failed login tracking", has_tracking)

def test_lockout_after_failed_attempts():
    """Test that account gets locked after N failed attempts."""
    print("\n=== Test 2: Lockout After Failed Attempts ===")

    # Make multiple failed login attempts
    locked = False
    for i in range(6):  # Default is 5 attempts before lockout
        response = requests.post(
            f"{BASE_URL}/api/v1/auth/login",
            json={"username": "lockout_victim", "password": "wrong_pass"}
        )
        result = response.json()
        print(f"  Attempt {i+1}: Status {response.status_code}, locked={result.get('locked', False)}")

        if result.get("locked"):
            locked = True
            break

    return print_test("Lockout after failed attempts", locked)

def test_locked_account_returns_429():
    """Test that locked account returns 429 Too Many Requests."""
    print("\n=== Test 3: Locked Account Returns 429 ===")

    # Try to login with the locked account
    response = requests.post(
        f"{BASE_URL}/api/v1/auth/login",
        json={"username": "lockout_victim", "password": "any_pass"}
    )

    is_429 = response.status_code == 429
    result = response.json()

    if is_429:
        print(f"  Error: {result.get('error')}")
        print(f"  Lockout expires: {result.get('lockout_expires')}")

    return print_test("Locked account returns 429", is_429)

def test_admin_get_locked_accounts(db):
    """Test admin can get list of locked accounts."""
    print("\n=== Test 4: Admin Get Locked Accounts ===")

    try:
        result = db.get_locked_accounts()
        print(f"  Locked accounts count: {result.get('count')}")

        locked_users = result.get('locked_users', [])
        for user in locked_users[:3]:  # Show first 3
            print(f"    - {user}")

        return print_test("Admin get locked accounts", True)
    except Exception as e:
        print(f"  Error: {e}")
        return print_test("Admin get locked accounts", False)

def test_admin_get_lockout_status(db):
    """Test admin can get lockout status for a specific user."""
    print("\n=== Test 5: Admin Get Lockout Status ===")

    try:
        result = db.get_lockout_status("lockout_victim")
        status = result.get('status', {})

        print(f"  Username: {result.get('username')}")
        print(f"  Locked: {status.get('locked')}")
        print(f"  Failed attempts: {status.get('failed_attempts')}")
        print(f"  Remaining attempts: {status.get('remaining_attempts')}")
        print(f"  Lockout count: {status.get('lockout_count')}")

        return print_test("Admin get lockout status", True)
    except Exception as e:
        print(f"  Error: {e}")
        return print_test("Admin get lockout status", False)

def test_admin_unlock_account(db):
    """Test admin can unlock a locked account."""
    print("\n=== Test 6: Admin Unlock Account ===")

    try:
        result = db.unlock_account("lockout_victim")
        print(f"  Result: {result}")

        success = result.get('success', False)
        return print_test("Admin unlock account", success)
    except Exception as e:
        print(f"  Error: {e}")
        return print_test("Admin unlock account", False)

def test_admin_manual_lock_account(db):
    """Test admin can manually lock an account."""
    print("\n=== Test 7: Admin Manual Lock Account ===")

    try:
        result = db.lock_account("lockout_victim", reason="Test manual lock")
        print(f"  Result: {result}")

        success = result.get('success', False)
        return print_test("Admin manual lock account", success)
    except Exception as e:
        print(f"  Error: {e}")
        return print_test("Admin manual lock account", False)

def test_manual_lock_persists(db):
    """Test that manual lock persists and returns 429."""
    print("\n=== Test 8: Manual Lock Persists ===")

    # Try to login with manually locked account
    response = requests.post(
        f"{BASE_URL}/api/v1/auth/login",
        json={"username": "lockout_victim", "password": "any_pass"}
    )

    is_locked = response.status_code == 429
    result = response.json()

    if is_locked:
        print(f"  Account is locked: {result.get('locked')}")
        print(f"  Lockout reason: {result.get('lockout_reason')}")

    return print_test("Manual lock persists", is_locked)

def test_sdk_lockout_methods(db):
    """Test all Python SDK lockout methods work correctly."""
    print("\n=== Test 9: SDK Lockout Methods Integration ===")

    all_passed = True

    # 1. Unlock the test account first
    try:
        db.unlock_account("lockout_victim")
        print("  Unlocked account")
    except:
        pass

    # 2. Get lockout status (should be unlocked)
    try:
        status = db.get_lockout_status("lockout_victim")
        is_unlocked = not status.get('status', {}).get('locked', True)
        print(f"  Status after unlock: locked={not is_unlocked}")
        all_passed = all_passed and is_unlocked
    except Exception as e:
        print(f"  Error getting status: {e}")
        all_passed = False

    # 3. Lock the account
    try:
        result = db.lock_account("lockout_victim", reason="SDK test")
        locked = result.get('success', False)
        print(f"  Lock account: success={locked}")
        all_passed = all_passed and locked
    except Exception as e:
        print(f"  Error locking: {e}")
        all_passed = False

    # 4. Verify it's in locked accounts list
    try:
        locked_list = db.get_locked_accounts()
        in_list = "lockout_victim" in str(locked_list.get('locked_users', []))
        print(f"  In locked accounts list: {in_list}")
        all_passed = all_passed and in_list
    except Exception as e:
        print(f"  Error getting list: {e}")
        all_passed = False

    # 5. Unlock again
    try:
        result = db.unlock_account("lockout_victim")
        unlocked = result.get('success', False)
        print(f"  Unlock account: success={unlocked}")
        all_passed = all_passed and unlocked
    except Exception as e:
        print(f"  Error unlocking: {e}")
        all_passed = False

    return print_test("SDK lockout methods integration", all_passed)

def main():
    """Run all tests."""
    print("=" * 60)
    print("Account Lockout Feature Tests")
    print("=" * 60)

    # Connect as admin
    print("\nConnecting as admin...")
    db = QilbeeDB(BASE_URL)
    try:
        db.login(ADMIN_USER, ADMIN_PASS)
        print("Admin login successful")
    except Exception as e:
        print(f"Admin login failed: {e}")
        return 1

    # Run tests
    results = []

    # Test 1: Failed login tracking
    results.append(test_failed_login_tracking())

    # Test 2: Lockout after failed attempts
    results.append(test_lockout_after_failed_attempts())

    # Test 3: Locked account returns 429
    results.append(test_locked_account_returns_429())

    # Test 4: Admin get locked accounts
    results.append(test_admin_get_locked_accounts(db))

    # Test 5: Admin get lockout status
    results.append(test_admin_get_lockout_status(db))

    # Test 6: Admin unlock account
    results.append(test_admin_unlock_account(db))

    # Test 7: Admin manual lock
    results.append(test_admin_manual_lock_account(db))

    # Test 8: Manual lock persists
    results.append(test_manual_lock_persists(db))

    # Test 9: SDK integration
    results.append(test_sdk_lockout_methods(db))

    # Summary
    print("\n" + "=" * 60)
    print("Test Summary")
    print("=" * 60)

    passed = sum(results)
    total = len(results)

    print(f"\nPassed: {passed}/{total}")

    if passed == total:
        print("\nAll tests passed!")
        return 0
    else:
        print(f"\n{total - passed} tests failed!")
        return 1

if __name__ == "__main__":
    sys.exit(main())
