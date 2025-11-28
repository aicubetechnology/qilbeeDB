#!/usr/bin/env python3
"""
Test API key expiration and rotation via the HTTP API.

This test verifies that the server supports:
- Creating API keys with optional expiration time
- API key rotation (atomically create new key and revoke old key)
- Proper expiration date handling
"""

import requests
import json
import sys
from datetime import datetime, timedelta

BASE_URL = "http://localhost:7474"
ADMIN_PASSWORD = "SecureAdmin@123!"  # Matches server bootstrap password


def get_admin_token():
    """Login as admin and get token."""
    response = requests.post(
        f"{BASE_URL}/api/v1/auth/login",
        json={"username": "admin", "password": ADMIN_PASSWORD}
    )
    if response.status_code != 200:
        print(f"Failed to login as admin: {response.status_code}")
        print(response.text)
        return None
    return response.json().get("access_token")


def test_create_api_key_without_expiration(token):
    """Test creating an API key without expiration."""
    headers = {"Authorization": f"Bearer {token}"}

    response = requests.post(
        f"{BASE_URL}/api/v1/api-keys",
        headers=headers,
        json={"name": "no-expiration-key"}
    )

    if response.status_code in (200, 201):
        data = response.json()
        if data.get("expires_at") is None:
            print("PASS: API key without expiration created successfully")
            return data.get("key"), data.get("id")
        else:
            print(f"FAIL: Expected no expiration, got expires_at={data.get('expires_at')}")
            return None, None
    else:
        print(f"FAIL: Could not create API key: {response.text}")
        return None, None


def test_create_api_key_with_expiration(token):
    """Test creating an API key with 30-day expiration."""
    headers = {"Authorization": f"Bearer {token}"}

    response = requests.post(
        f"{BASE_URL}/api/v1/api-keys",
        headers=headers,
        json={
            "name": "expiring-key",
            "expires_in_days": 30
        }
    )

    if response.status_code in (200, 201):
        data = response.json()
        expires_at = data.get("expires_at")
        if expires_at:
            # Parse the expiration date
            try:
                exp_date = datetime.fromisoformat(expires_at.replace("Z", "+00:00"))
                now = datetime.now(exp_date.tzinfo)
                days_until_expiration = (exp_date - now).days

                if 28 <= days_until_expiration <= 31:  # Allow some tolerance
                    print(f"PASS: API key with expiration created (expires in ~{days_until_expiration} days)")
                    return data.get("key"), data.get("id")
                else:
                    print(f"FAIL: Expected ~30 days until expiration, got {days_until_expiration}")
                    return None, None
            except Exception as e:
                print(f"FAIL: Could not parse expiration date: {e}")
                return None, None
        else:
            print("FAIL: Expected expires_at to be set")
            return None, None
    else:
        print(f"FAIL: Could not create API key: {response.text}")
        return None, None


def test_api_key_rotation(token, old_key):
    """Test rotating an API key."""
    headers = {"Authorization": f"Bearer {token}"}

    response = requests.post(
        f"{BASE_URL}/api/v1/api-keys/rotate",
        headers=headers,
        json={
            "current_key": old_key,
            "new_name": "rotated-key",
            "expires_in_days": 90
        }
    )

    if response.status_code in (200, 201):
        data = response.json()
        new_key = data.get("key")
        expires_at = data.get("expires_at")

        if new_key and new_key.startswith("qilbee_live_"):
            print("PASS: API key rotation returned new key with correct prefix")
        else:
            print(f"FAIL: New key has invalid format: {new_key[:20] if new_key else 'None'}...")
            return None, None

        if data.get("name") == "rotated-key":
            print("PASS: Rotated key has new name")
        else:
            print(f"FAIL: Expected name 'rotated-key', got {data.get('name')}")

        if expires_at:
            print("PASS: Rotated key has expiration set")
        else:
            print("FAIL: Expected expiration to be set")

        return new_key, data.get("id")
    else:
        print(f"FAIL: Could not rotate API key: {response.text}")
        return None, None


def test_old_key_no_longer_works(old_key):
    """Test that the old key no longer works after rotation."""
    headers = {"X-API-Key": old_key}

    response = requests.get(
        f"{BASE_URL}/api/v1/users",
        headers=headers
    )

    if response.status_code == 401 or response.status_code == 403:
        print("PASS: Old API key is no longer valid after rotation")
        return True
    else:
        print(f"FAIL: Old API key still works (status {response.status_code})")
        return False


def test_new_key_works(new_key):
    """Test that the new key works after rotation."""
    headers = {"X-API-Key": new_key}

    response = requests.get(
        f"{BASE_URL}/api/v1/users",
        headers=headers
    )

    if response.status_code == 200:
        print("PASS: New API key works correctly")
        return True
    else:
        print(f"FAIL: New API key doesn't work (status {response.status_code}): {response.text}")
        return False


def test_rotation_with_invalid_key(token):
    """Test that rotation fails with an invalid key."""
    headers = {"Authorization": f"Bearer {token}"}

    response = requests.post(
        f"{BASE_URL}/api/v1/api-keys/rotate",
        headers=headers,
        json={
            "current_key": "invalid_key_12345",
            "new_name": "should-fail"
        }
    )

    if response.status_code >= 400:
        print("PASS: Rotation with invalid key rejected")
        return True
    else:
        print(f"FAIL: Rotation with invalid key should have failed: {response.text}")
        return False


def cleanup_api_key(token, key_id):
    """Delete an API key by ID."""
    if not key_id:
        return
    headers = {"Authorization": f"Bearer {token}"}
    requests.delete(f"{BASE_URL}/api/v1/api-keys/{key_id}", headers=headers)


def main():
    print("=" * 60)
    print("API Key Expiration and Rotation Tests")
    print("=" * 60)
    print()

    # Get admin token
    token = get_admin_token()
    if not token:
        print("ERROR: Could not get admin token. Is the server running?")
        return 1

    print("Successfully authenticated as admin")
    print()

    all_passed = True
    created_key_ids = []

    # Test 1: Create API key without expiration
    print("-" * 40)
    print("Test 1: Create API key without expiration")
    print("-" * 40)
    key1, key_id1 = test_create_api_key_without_expiration(token)
    if key_id1:
        created_key_ids.append(key_id1)
    if not key1:
        all_passed = False
    print()

    # Test 2: Create API key with expiration
    print("-" * 40)
    print("Test 2: Create API key with 30-day expiration")
    print("-" * 40)
    key2, key_id2 = test_create_api_key_with_expiration(token)
    if key_id2:
        created_key_ids.append(key_id2)
    if not key2:
        all_passed = False
    print()

    # Test 3: Rotate API key
    print("-" * 40)
    print("Test 3: Rotate API key")
    print("-" * 40)
    if key1:
        new_key, new_key_id = test_api_key_rotation(token, key1)
        if new_key_id:
            created_key_ids.append(new_key_id)
            # Remove old key ID since it was deleted during rotation
            if key_id1 in created_key_ids:
                created_key_ids.remove(key_id1)
        if not new_key:
            all_passed = False
    else:
        print("SKIP: No key available for rotation test")
        new_key = None
    print()

    # Test 4: Old key no longer works
    print("-" * 40)
    print("Test 4: Old key invalidated after rotation")
    print("-" * 40)
    if key1:
        if not test_old_key_no_longer_works(key1):
            all_passed = False
    else:
        print("SKIP: No old key to test")
    print()

    # Test 5: New key works
    print("-" * 40)
    print("Test 5: New key works after rotation")
    print("-" * 40)
    if new_key:
        if not test_new_key_works(new_key):
            all_passed = False
    else:
        print("SKIP: No new key to test")
    print()

    # Test 6: Rotation with invalid key fails
    print("-" * 40)
    print("Test 6: Rotation with invalid key fails")
    print("-" * 40)
    if not test_rotation_with_invalid_key(token):
        all_passed = False
    print()

    # Cleanup
    print("-" * 40)
    print("Cleanup: Deleting test API keys")
    print("-" * 40)
    for key_id in created_key_ids:
        cleanup_api_key(token, key_id)
    print(f"Cleaned up {len(created_key_ids)} API keys")
    print()

    print("=" * 60)
    if all_passed:
        print("All API key rotation tests PASSED!")
        return 0
    else:
        print("Some API key rotation tests FAILED!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
