#!/usr/bin/env python3
"""
Test rate limit enforcement with custom low-limit policy.
"""

import requests
import time
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'sdks', 'python'))

from qilbeedb import QilbeeDB

BASE_URL = "http://localhost:7474"

print("=" * 70)
print("Rate Limit Test - Custom Policy with Low Limit")
print("=" * 70)

# Step 1: Login as admin and create a restrictive policy
print("\n[Step 1] Creating low-limit policy (5 requests per 60 sec)...")
db = QilbeeDB(BASE_URL)
try:
    db.login("admin", "Admin123!@#")
    print("  OK: Logged in as admin")
except Exception as e:
    print(f"  FAIL: {e}")
    sys.exit(1)

policy_id = None
try:
    policy = db.create_rate_limit_policy(
        name="Test Low Limit",
        endpoint_type="Login",
        max_requests=5,
        window_secs=60,
        enabled=True
    )
    policy_id = policy["id"]
    print(f"  OK: Created policy {policy_id}")
    print(f"      Limit: 5 requests per 60 seconds")
except Exception as e:
    print(f"  FAIL: Could not create policy: {e}")

# Wait a moment for policy to take effect
time.sleep(1)

# Step 2: Make requests until we hit 429
print("\n[Step 2] Testing rate limit enforcement...")
print("  Making rapid login requests (expecting 429 after 5)...")
print()

rate_limited = False
for i in range(1, 12):
    try:
        response = requests.post(
            f"{BASE_URL}/api/v1/auth/login",
            json={"username": "testuser", "password": "wrongpass"},
            timeout=5
        )

        status = response.status_code
        limit = response.headers.get("X-RateLimit-Limit", "?")
        remaining = response.headers.get("X-RateLimit-Remaining", "?")
        reset = response.headers.get("X-RateLimit-Reset", "?")

        if status == 429:
            rate_limited = True
            print(f"  Request {i:2d}: 429 RATE LIMITED | Limit={limit}, Remaining={remaining}, Reset={reset}s")
        else:
            print(f"  Request {i:2d}: {status} OK | Limit={limit}, Remaining={remaining}, Reset={reset}s")

    except Exception as e:
        print(f"  Request {i:2d}: ERROR - {e}")

    time.sleep(0.05)

# Step 3: Results
print("\n" + "=" * 70)
print("Results")
print("=" * 70)

if rate_limited:
    print("\nSUCCESS: Rate limiting is enforced!")
    print("  - Custom policy with 5 req/min was applied")
    print("  - 429 response was returned when limit exceeded")
else:
    print("\nPROBLEM: No 429 response received!")
    print("  Possible causes:")
    print("  - Custom policy not overriding default policy")
    print("  - Policy lookup logic issue")

# Cleanup
print("\n[Cleanup] Removing test policy...")
if policy_id:
    try:
        db.delete_rate_limit_policy(policy_id)
        print(f"  OK: Deleted policy {policy_id}")
    except Exception as e:
        print(f"  WARN: Could not delete: {e}")

db.close()
print("\nDone.")
