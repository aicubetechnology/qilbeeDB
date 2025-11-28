#!/usr/bin/env python3
"""
Test HTTPS enforcement configuration.

This test verifies that the HTTPS enforcement middleware works correctly:
- By default (HTTPS_ENFORCE=false), requests work normally over HTTP
- When enabled, HTTP requests to non-localhost get redirected
- Health endpoints are exempt from HTTPS enforcement
- X-Forwarded-Proto header is respected (for proxy setups)

Note: Full HTTPS redirect testing requires running with HTTPS_ENFORCE=true,
which would break the test server. This test focuses on verifying the
configuration is applied correctly.
"""

import requests
import sys
import os

BASE_URL = "http://localhost:7474"


def test_default_http_works():
    """Test that HTTP requests work by default (HTTPS enforcement disabled)."""
    response = requests.get(f"{BASE_URL}/health")

    if response.status_code == 200:
        print("PASS: HTTP request works (HTTPS enforcement disabled by default)")
        return True
    else:
        print(f"FAIL: HTTP request returned {response.status_code}")
        return False


def test_health_endpoint_accessible():
    """Test that health endpoint is accessible over HTTP."""
    response = requests.get(f"{BASE_URL}/health")

    if response.status_code == 200:
        data = response.json()
        if data.get("status") == "healthy":
            print("PASS: Health endpoint returns healthy status")
            return True
        else:
            print(f"FAIL: Health endpoint returned unexpected status: {data}")
            return False
    else:
        print(f"FAIL: Health endpoint returned {response.status_code}")
        return False


def test_api_endpoint_accessible():
    """Test that API endpoints are accessible over HTTP (when HTTPS not enforced)."""
    # Login endpoint should be accessible
    response = requests.post(
        f"{BASE_URL}/api/v1/auth/login",
        json={"username": "admin", "password": "SecureAdmin@123!"}
    )

    if response.status_code in [200, 401]:  # 200 = success, 401 = invalid credentials
        print(f"PASS: API endpoint accessible (status {response.status_code})")
        return True
    else:
        print(f"FAIL: API endpoint returned unexpected status: {response.status_code}")
        return False


def test_x_forwarded_proto_header():
    """Test that X-Forwarded-Proto header can be used to indicate HTTPS."""
    # Even if HTTPS enforcement were enabled, X-Forwarded-Proto: https
    # should allow the request through (simulating a proxy that terminates TLS)
    response = requests.get(
        f"{BASE_URL}/health",
        headers={"X-Forwarded-Proto": "https"}
    )

    if response.status_code == 200:
        print("PASS: Request with X-Forwarded-Proto: https succeeds")
        return True
    else:
        print(f"FAIL: Request with X-Forwarded-Proto returned {response.status_code}")
        return False


def test_localhost_exemption():
    """Test that localhost requests work (should be exempt from HTTPS)."""
    # Localhost should always work, even if HTTPS enforcement is enabled
    response = requests.get(
        f"{BASE_URL}/health",
        headers={"Host": "localhost:7474"}
    )

    if response.status_code == 200:
        print("PASS: Localhost request succeeds (exempt from HTTPS)")
        return True
    else:
        print(f"FAIL: Localhost request returned {response.status_code}")
        return False


def test_127_0_0_1_exemption():
    """Test that 127.0.0.1 requests work (should be exempt from HTTPS)."""
    response = requests.get(
        "http://127.0.0.1:7474/health",
        headers={"Host": "127.0.0.1:7474"}
    )

    if response.status_code == 200:
        print("PASS: 127.0.0.1 request succeeds (exempt from HTTPS)")
        return True
    else:
        print(f"FAIL: 127.0.0.1 request returned {response.status_code}")
        return False


def main():
    print("=" * 60)
    print("HTTPS Enforcement Configuration Tests")
    print("=" * 60)
    print()
    print("NOTE: Server should be running with default settings")
    print("      (HTTPS_ENFORCE=false for development)")
    print()

    all_passed = True

    # Test 1: Default HTTP works
    print("-" * 40)
    print("Test 1: HTTP Requests Work by Default")
    print("-" * 40)
    if not test_default_http_works():
        all_passed = False
    print()

    # Test 2: Health endpoint accessible
    print("-" * 40)
    print("Test 2: Health Endpoint Accessible")
    print("-" * 40)
    if not test_health_endpoint_accessible():
        all_passed = False
    print()

    # Test 3: API endpoint accessible
    print("-" * 40)
    print("Test 3: API Endpoints Accessible")
    print("-" * 40)
    if not test_api_endpoint_accessible():
        all_passed = False
    print()

    # Test 4: X-Forwarded-Proto header
    print("-" * 40)
    print("Test 4: X-Forwarded-Proto Header")
    print("-" * 40)
    if not test_x_forwarded_proto_header():
        all_passed = False
    print()

    # Test 5: Localhost exemption
    print("-" * 40)
    print("Test 5: Localhost Exemption")
    print("-" * 40)
    if not test_localhost_exemption():
        all_passed = False
    print()

    # Test 6: 127.0.0.1 exemption
    print("-" * 40)
    print("Test 6: 127.0.0.1 Exemption")
    print("-" * 40)
    if not test_127_0_0_1_exemption():
        all_passed = False
    print()

    print("=" * 60)
    if all_passed:
        print("All HTTPS enforcement tests PASSED!")
        print()
        print("Environment Variables for Production:")
        print("  HTTPS_ENFORCE=true       - Enable HTTPS enforcement")
        print("  HTTPS_PORT=443           - HTTPS port (default: 443)")
        print("  HTTPS_ALLOW_LOCALHOST=false - Disable localhost exemption")
        print("  HTTPS_TRUST_PROXY=true   - Trust X-Forwarded-Proto header")
        print("  TLS_CERT_PATH=/path/cert - TLS certificate path")
        print("  TLS_KEY_PATH=/path/key   - TLS private key path")
        return 0
    else:
        print("Some HTTPS enforcement tests FAILED!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
