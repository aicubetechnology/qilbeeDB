#!/usr/bin/env python3
"""
Focused test to verify rate limit enforcement on the login endpoint.

This test will:
1. First create a low-limit policy (5 req/min) for the login endpoint
2. Make rapid login requests to verify they get blocked
3. Check for proper 429 response and rate limit headers
"""

import requests
import time
import sys
import os

# Add SDK to path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'sdks', 'python'))

from qilbeedb import QilbeeDB

BASE_URL = "http://localhost:7474"

print("=" * 70)
print("Rate Limit Verification Test - Login Endpoint")
print("=" * 70)

# Step 1: Login as admin and create a restrictive policy
print("\n[Step 1] Setting up test policy...")
db = QilbeeDB(BASE_URL)
try:
    db.login("admin", "Admin123!@#")
    print("  ✓ Logged in as admin")
except Exception as e:
    print(f"  ✗ Failed to login: {e}")
    sys.exit(1)

# Create a very restrictive policy (5 requests per minute)
try:
    policy = db.create_rate_limit_policy(
        name="Test Login Limit 5rpm",
        endpoint_type="Login",
        max_requests=5,
        window_secs=60,
        enabled=True
    )
    policy_id = policy["id"]
    print(f"  ✓ Created policy: {policy_id}")
    print(f"    Limit: 5 requests per 60 seconds")
except Exception as e:
    print(f"  ✗ Failed to create policy: {e}")
    # Continue anyway - default policy should still work

# Step 2: Make direct HTTP requests to track headers
print("\n[Step 2] Testing rate limit enforcement...")
print("  Making 10 rapid login requests...")
print()

rate_limited = False
last_rate_limit_headers = {}

for i in range(1, 11):
    try:
        response = requests.post(
            f"{BASE_URL}/api/v1/auth/login",
            json={"username": "testuser", "password": "wrongpass"},
            timeout=5
        )

        # Capture rate limit headers
        headers = {
            "X-RateLimit-Limit": response.headers.get("X-RateLimit-Limit", "N/A"),
            "X-RateLimit-Remaining": response.headers.get("X-RateLimit-Remaining", "N/A"),
            "X-RateLimit-Reset": response.headers.get("X-RateLimit-Reset", "N/A"),
        }
        last_rate_limit_headers = headers

        if response.status_code == 429:
            rate_limited = True
            print(f"  Request {i:2d}: 429 TOO MANY REQUESTS [Rate limited!]")
            print(f"             Headers: Limit={headers['X-RateLimit-Limit']}, "
                  f"Remaining={headers['X-RateLimit-Remaining']}, "
                  f"Reset={headers['X-RateLimit-Reset']}s")
        elif response.status_code == 401:
            # Expected - wrong password
            print(f"  Request {i:2d}: 401 Unauthorized (expected - wrong password)")
            if headers['X-RateLimit-Limit'] != 'N/A':
                print(f"             Headers: Limit={headers['X-RateLimit-Limit']}, "
                      f"Remaining={headers['X-RateLimit-Remaining']}, "
                      f"Reset={headers['X-RateLimit-Reset']}s")
            else:
                print(f"             Headers: NO RATE LIMIT HEADERS FOUND!")
        else:
            print(f"  Request {i:2d}: {response.status_code} - {response.text[:100]}")

    except Exception as e:
        print(f"  Request {i:2d}: ERROR - {e}")

    # Very small delay to avoid overwhelming
    time.sleep(0.05)

# Step 3: Analyze results
print("\n" + "=" * 70)
print("Results")
print("=" * 70)

# Check if rate limit headers were present
if last_rate_limit_headers.get('X-RateLimit-Limit', 'N/A') != 'N/A':
    print("\n✓ Rate limit headers are present in responses")
else:
    print("\n✗ Rate limit headers NOT found in responses")
    print("  This indicates the rate_limit middleware is NOT being applied!")

if rate_limited:
    print("✓ Rate limiting IS enforced - received 429 responses")
else:
    print("✗ Rate limiting NOT enforced - no 429 responses received")
    print("  All 10 requests were allowed to proceed!")

# Cleanup
print("\n[Cleanup] Removing test policy...")
try:
    db.delete_rate_limit_policy(policy_id)
    print("  ✓ Deleted test policy")
except Exception as e:
    print(f"  ⚠ Could not delete policy: {e}")

db.close()

print("\n" + "=" * 70)
if rate_limited and last_rate_limit_headers.get('X-RateLimit-Limit', 'N/A') != 'N/A':
    print("PASS: Rate limiting is working correctly on login endpoint")
else:
    print("FAIL: Rate limiting is NOT working properly")
    print("\nPotential issues:")
    print("1. Middleware not applied to the route")
    print("2. Policy not being found for EndpointType::Login")
    print("3. Token bucket not being consumed correctly")
print("=" * 70)
