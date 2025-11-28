#!/usr/bin/env python3
"""
Test security headers in HTTP responses.

This test verifies that the server includes all required security headers
in HTTP responses to protect against common web vulnerabilities.
"""

import requests
import sys

BASE_URL = "http://localhost:7474"

# Expected security headers and their values
EXPECTED_HEADERS = {
    "x-content-type-options": "nosniff",
    "x-frame-options": "DENY",
    "x-xss-protection": "1; mode=block",
    "referrer-policy": "strict-origin-when-cross-origin",
    "cache-control": "no-store, no-cache, must-revalidate, proxy-revalidate",
    "pragma": "no-cache",
    "x-permitted-cross-domain-policies": "none",
    "x-download-options": "noopen",
}

# Headers that should be present (we check they exist, not exact value)
EXPECTED_PRESENT_HEADERS = [
    "strict-transport-security",
    "content-security-policy",
    "permissions-policy",
]


def test_security_headers_on_health():
    """Test security headers on health endpoint."""
    response = requests.get(f"{BASE_URL}/health")

    if response.status_code != 200:
        print(f"FAIL: Health endpoint returned {response.status_code}")
        return False

    all_passed = True

    # Check exact value headers
    for header, expected_value in EXPECTED_HEADERS.items():
        actual_value = response.headers.get(header)
        if actual_value is None:
            print(f"FAIL: Missing header: {header}")
            all_passed = False
        elif actual_value != expected_value:
            print(f"FAIL: Header '{header}' has value '{actual_value}', expected '{expected_value}'")
            all_passed = False
        else:
            print(f"PASS: {header}: {actual_value}")

    # Check presence of headers
    for header in EXPECTED_PRESENT_HEADERS:
        actual_value = response.headers.get(header)
        if actual_value is None:
            print(f"FAIL: Missing header: {header}")
            all_passed = False
        else:
            print(f"PASS: {header} is present")

    return all_passed


def test_security_headers_on_api():
    """Test security headers on API endpoint."""
    response = requests.post(
        f"{BASE_URL}/api/v1/auth/login",
        json={"username": "invalid", "password": "invalid"}
    )

    # Even on 401, security headers should be present
    all_passed = True

    for header in ["x-content-type-options", "x-frame-options", "strict-transport-security"]:
        if header not in response.headers:
            print(f"FAIL: API response missing header: {header}")
            all_passed = False
        else:
            print(f"PASS: API response has header: {header}")

    return all_passed


def test_hsts_configuration():
    """Test HSTS header configuration."""
    response = requests.get(f"{BASE_URL}/health")

    hsts = response.headers.get("strict-transport-security")
    if hsts is None:
        print("FAIL: HSTS header missing")
        return False

    # Check for required directives
    if "max-age=" not in hsts:
        print(f"FAIL: HSTS missing max-age directive: {hsts}")
        return False
    print("PASS: HSTS has max-age directive")

    if "includeSubDomains" not in hsts:
        print(f"WARN: HSTS missing includeSubDomains (recommended): {hsts}")
    else:
        print("PASS: HSTS has includeSubDomains")

    return True


def test_csp_configuration():
    """Test Content-Security-Policy header configuration."""
    response = requests.get(f"{BASE_URL}/health")

    csp = response.headers.get("content-security-policy")
    if csp is None:
        print("FAIL: CSP header missing")
        return False

    # Check for essential directives
    essential_directives = ["default-src", "script-src", "frame-ancestors"]
    all_passed = True

    for directive in essential_directives:
        if directive not in csp:
            print(f"FAIL: CSP missing directive: {directive}")
            all_passed = False
        else:
            print(f"PASS: CSP has {directive} directive")

    return all_passed


def test_permissions_policy():
    """Test Permissions-Policy header configuration."""
    response = requests.get(f"{BASE_URL}/health")

    policy = response.headers.get("permissions-policy")
    if policy is None:
        print("FAIL: Permissions-Policy header missing")
        return False

    # Check that dangerous APIs are disabled
    disabled_apis = ["geolocation", "microphone", "camera"]
    all_passed = True

    for api in disabled_apis:
        if api not in policy:
            print(f"WARN: Permissions-Policy doesn't explicitly disable: {api}")
        else:
            print(f"PASS: Permissions-Policy addresses {api}")

    return all_passed


def main():
    print("=" * 60)
    print("Security Headers Tests")
    print("=" * 60)
    print()

    all_passed = True

    # Test 1: Security headers on health endpoint
    print("-" * 40)
    print("Test 1: Security headers on /health endpoint")
    print("-" * 40)
    if not test_security_headers_on_health():
        all_passed = False
    print()

    # Test 2: Security headers on API endpoint
    print("-" * 40)
    print("Test 2: Security headers on API endpoint")
    print("-" * 40)
    if not test_security_headers_on_api():
        all_passed = False
    print()

    # Test 3: HSTS configuration
    print("-" * 40)
    print("Test 3: HSTS configuration")
    print("-" * 40)
    if not test_hsts_configuration():
        all_passed = False
    print()

    # Test 4: CSP configuration
    print("-" * 40)
    print("Test 4: Content-Security-Policy configuration")
    print("-" * 40)
    if not test_csp_configuration():
        all_passed = False
    print()

    # Test 5: Permissions Policy
    print("-" * 40)
    print("Test 5: Permissions-Policy configuration")
    print("-" * 40)
    if not test_permissions_policy():
        all_passed = False
    print()

    print("=" * 60)
    if all_passed:
        print("All security headers tests PASSED!")
        return 0
    else:
        print("Some security headers tests FAILED!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
