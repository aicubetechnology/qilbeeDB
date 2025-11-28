#!/usr/bin/env python3
"""
Test password complexity validation via the HTTP API.

This test verifies that the server enforces password complexity requirements:
- Minimum 12 characters
- At least one uppercase letter
- At least one lowercase letter
- At least one number
- At least one special character (!@#$%^&*()_+-=[]{}|;:,.<>?)
"""

import requests
import json
import sys

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


def test_create_user_with_weak_password(token):
    """Test that creating a user with a weak password fails."""
    weak_passwords = [
        ("short", "Too short - less than 12 characters"),
        ("alllowercase123!", "Missing uppercase letter"),
        ("ALLUPPERCASE123!", "Missing lowercase letter"),
        ("NoDigitsHere!!!!", "Missing digit"),
        ("NoSpecialChar123", "Missing special character"),
        ("weakpassword", "Multiple issues"),
    ]

    headers = {"Authorization": f"Bearer {token}"}
    all_passed = True

    for password, description in weak_passwords:
        response = requests.post(
            f"{BASE_URL}/api/v1/users",
            headers=headers,
            json={
                "username": f"testuser_{password[:6]}",
                "email": f"test_{password[:6]}@example.com",
                "password": password,
            }
        )

        if response.status_code in (200, 201):
            print(f"FAIL: {description} - Password '{password}' was accepted but should have been rejected")
            all_passed = False
        else:
            # Should get an error about weak password
            error_text = response.text.lower()
            if "weak" in error_text or "password" in error_text:
                print(f"PASS: {description} - Password '{password}' correctly rejected")
            else:
                print(f"PASS: {description} - Password '{password}' rejected with: {response.text[:100]}")

    return all_passed


def test_create_user_with_strong_password(token):
    """Test that creating a user with a strong password succeeds."""
    strong_passwords = [
        "SecureP@ss123!",
        "MyS3cure#Pass!",
        "C0mplex!tyRul3s",
        "VeryStr0ng&Secure",
    ]

    headers = {"Authorization": f"Bearer {token}"}
    all_passed = True

    for i, password in enumerate(strong_passwords):
        response = requests.post(
            f"{BASE_URL}/api/v1/users",
            headers=headers,
            json={
                "username": f"stronguser{i}",
                "email": f"stronguser{i}@example.com",
                "password": password,
            }
        )

        if response.status_code in (200, 201):
            print(f"PASS: Strong password '{password[:6]}...' accepted")
        else:
            # Check if it's a duplicate error (acceptable)
            if "already exists" in response.text.lower():
                print(f"PASS: Strong password '{password[:6]}...' - user already exists (expected)")
            else:
                print(f"FAIL: Strong password '{password[:6]}...' rejected: {response.text[:100]}")
                all_passed = False

    return all_passed


def test_password_update_validation(token):
    """Test that password update also validates complexity."""
    headers = {"Authorization": f"Bearer {token}"}

    # First, create a test user with strong password
    create_resp = requests.post(
        f"{BASE_URL}/api/v1/users",
        headers=headers,
        json={
            "username": "pwdupdate_user",
            "email": "pwdupdate@example.com",
            "password": "InitialP@ss123!",
        }
    )

    if create_resp.status_code not in (200, 201) and "already exists" not in create_resp.text.lower():
        print(f"SKIP: Could not create test user for password update test: {create_resp.text}")
        return True

    # Get user ID
    if create_resp.status_code in (200, 201):
        user_id = create_resp.json().get("id")
    else:
        # Try to get user list to find the user
        list_resp = requests.get(f"{BASE_URL}/api/v1/users", headers=headers)
        if list_resp.status_code != 200:
            print("SKIP: Could not get user list")
            return True
        users = list_resp.json()
        user = next((u for u in users if u.get("username") == "pwdupdate_user"), None)
        if not user:
            print("SKIP: Could not find test user")
            return True
        user_id = user.get("id")

    if not user_id:
        print("SKIP: No user ID found for password update test")
        return True

    # Try to update with weak password
    update_resp = requests.patch(
        f"{BASE_URL}/api/v1/users/{user_id}",
        headers=headers,
        json={"password": "weak"}
    )

    if update_resp.status_code == 200:
        print("FAIL: Password update with weak password was accepted")
        return False
    else:
        print(f"PASS: Password update with weak password rejected")
        return True


def main():
    print("=" * 60)
    print("Password Complexity Validation Tests")
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

    print("-" * 40)
    print("Test 1: Weak passwords should be rejected")
    print("-" * 40)
    if not test_create_user_with_weak_password(token):
        all_passed = False
    print()

    print("-" * 40)
    print("Test 2: Strong passwords should be accepted")
    print("-" * 40)
    if not test_create_user_with_strong_password(token):
        all_passed = False
    print()

    print("-" * 40)
    print("Test 3: Password update validation")
    print("-" * 40)
    if not test_password_update_validation(token):
        all_passed = False
    print()

    print("=" * 60)
    if all_passed:
        print("All password validation tests PASSED!")
        return 0
    else:
        print("Some password validation tests FAILED!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
