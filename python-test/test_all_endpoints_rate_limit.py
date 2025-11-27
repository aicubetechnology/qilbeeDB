#!/usr/bin/env python3
"""
Test rate limit headers are present on all endpoints.

This test verifies that rate limiting middleware is applied to all endpoints.
"""

import requests
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'sdks', 'python'))

from qilbeedb import QilbeeDB

BASE_URL = "http://localhost:7474"

print("=" * 70)
print("Rate Limit Headers Test - All Endpoints")
print("=" * 70)

# Login to get token
try:
    login_response = requests.post(f"{BASE_URL}/api/v1/auth/login",
                                   json={"username": "admin", "password": "Admin123!@#"})
    if login_response.status_code != 200:
        print(f"FAIL: Login failed - {login_response.text}")
        sys.exit(1)
    token = login_response.json()["access_token"]
    print(f"OK: Logged in as admin")
except Exception as e:
    print(f"FAIL: {e}")
    sys.exit(1)

db = None  # Not using SDK for this test

headers = {
    "Authorization": f"Bearer {token}",
    "Content-Type": "application/json"
}

def check_rate_limit_headers(name, response):
    """Check if rate limit headers are present."""
    limit = response.headers.get("X-RateLimit-Limit")
    remaining = response.headers.get("X-RateLimit-Remaining")
    reset = response.headers.get("X-RateLimit-Reset")

    if limit and remaining is not None and reset is not None:
        print(f"  OK: {name} - Limit={limit}, Remaining={remaining}, Reset={reset}s")
        return True
    else:
        print(f"  FAIL: {name} - No rate limit headers! (Status: {response.status_code})")
        return False

results = []

print("\n[1] Testing Auth Endpoints...")

# Login (already tested above, but let's verify headers)
r = requests.post(f"{BASE_URL}/api/v1/auth/login",
                  json={"username": "testuser", "password": "wrong"})
results.append(("Login", check_rate_limit_headers("POST /api/v1/auth/login", r)))

# Logout
r = requests.post(f"{BASE_URL}/api/v1/auth/logout",
                  json={"user_id": "test"}, headers=headers)
results.append(("Logout", check_rate_limit_headers("POST /api/v1/auth/logout", r)))

# Refresh
r = requests.post(f"{BASE_URL}/api/v1/auth/refresh",
                  json={"refresh_token": "invalid"}, headers=headers)
results.append(("Refresh", check_rate_limit_headers("POST /api/v1/auth/refresh", r)))

print("\n[2] Testing API Key Endpoints...")

# Create API Key
r = requests.post(f"{BASE_URL}/api/v1/api-keys",
                  json={"name": "test-key"}, headers=headers)
results.append(("Create API Key", check_rate_limit_headers("POST /api/v1/api-keys", r)))

# List API Keys
r = requests.get(f"{BASE_URL}/api/v1/api-keys", headers=headers)
results.append(("List API Keys", check_rate_limit_headers("GET /api/v1/api-keys", r)))

# Delete API Key (use fake id)
r = requests.delete(f"{BASE_URL}/api/v1/api-keys/00000000-0000-0000-0000-000000000000", headers=headers)
results.append(("Delete API Key", check_rate_limit_headers("DELETE /api/v1/api-keys/:id", r)))

print("\n[3] Testing User Management Endpoints...")

# List Users
r = requests.get(f"{BASE_URL}/api/v1/users", headers=headers)
results.append(("List Users", check_rate_limit_headers("GET /api/v1/users", r)))

# Create User (will fail due to duplicate, but should have headers)
r = requests.post(f"{BASE_URL}/api/v1/users",
                  json={"username": "testuser", "email": "test@test.com", "password": "Test123!"},
                  headers=headers)
results.append(("Create User", check_rate_limit_headers("POST /api/v1/users", r)))

# Get User (fake id)
r = requests.get(f"{BASE_URL}/api/v1/users/00000000-0000-0000-0000-000000000000", headers=headers)
results.append(("Get User", check_rate_limit_headers("GET /api/v1/users/:id", r)))

# Update User Roles (fake id)
r = requests.put(f"{BASE_URL}/api/v1/users/00000000-0000-0000-0000-000000000000/roles",
                 json={"roles": ["Read"]}, headers=headers)
results.append(("Update Roles", check_rate_limit_headers("PUT /api/v1/users/:id/roles", r)))

print("\n[4] Testing Rate Limit Policy Endpoints...")

# List Policies
r = requests.get(f"{BASE_URL}/api/v1/rate-limits", headers=headers)
results.append(("List Policies", check_rate_limit_headers("GET /api/v1/rate-limits", r)))

# Get Policy (fake id)
r = requests.get(f"{BASE_URL}/api/v1/rate-limits/00000000-0000-0000-0000-000000000000", headers=headers)
results.append(("Get Policy", check_rate_limit_headers("GET /api/v1/rate-limits/:id", r)))

print("\n[5] Testing Graph Endpoints...")

# Create Graph
r = requests.post(f"{BASE_URL}/graphs/test_rate_limit", headers=headers)
results.append(("Create Graph", check_rate_limit_headers("POST /graphs/:name", r)))

# Create Node
r = requests.post(f"{BASE_URL}/graphs/test_rate_limit/nodes",
                  json={"labels": ["Test"], "properties": {"name": "test"}}, headers=headers)
results.append(("Create Node", check_rate_limit_headers("POST /graphs/:name/nodes", r)))

# Find Nodes
r = requests.get(f"{BASE_URL}/graphs/test_rate_limit/nodes", headers=headers)
results.append(("Find Nodes", check_rate_limit_headers("GET /graphs/:name/nodes", r)))

# Get Node
r = requests.get(f"{BASE_URL}/graphs/test_rate_limit/nodes/1", headers=headers)
results.append(("Get Node", check_rate_limit_headers("GET /graphs/:name/nodes/:id", r)))

# Execute Query
r = requests.post(f"{BASE_URL}/graphs/test_rate_limit/query",
                  json={"cypher": "MATCH (n) RETURN n LIMIT 1"}, headers=headers)
results.append(("Execute Query", check_rate_limit_headers("POST /graphs/:name/query", r)))

# Delete Graph (cleanup)
r = requests.delete(f"{BASE_URL}/graphs/test_rate_limit", headers=headers)
results.append(("Delete Graph", check_rate_limit_headers("DELETE /graphs/:name", r)))

print("\n[6] Testing Memory Endpoints...")

# Store Episode
r = requests.post(f"{BASE_URL}/memory/test_agent/episodes",
                  json={"agentId": "test_agent", "episodeType": "conversation",
                        "content": {"primary": "test"}}, headers=headers)
results.append(("Store Episode", check_rate_limit_headers("POST /memory/:agent_id/episodes", r)))

# Get Recent Episodes
r = requests.get(f"{BASE_URL}/memory/test_agent/episodes/recent", headers=headers)
results.append(("Recent Episodes", check_rate_limit_headers("GET /memory/:agent_id/episodes/recent", r)))

# Get Statistics
r = requests.get(f"{BASE_URL}/memory/test_agent/statistics", headers=headers)
results.append(("Memory Stats", check_rate_limit_headers("GET /memory/:agent_id/statistics", r)))

# Search Episodes
r = requests.post(f"{BASE_URL}/memory/test_agent/episodes/search",
                  json={"query": "test"}, headers=headers)
results.append(("Search Episodes", check_rate_limit_headers("POST /memory/:agent_id/episodes/search", r)))

# Consolidate Memory
r = requests.post(f"{BASE_URL}/memory/test_agent/consolidate",
                  json={}, headers=headers)
results.append(("Consolidate", check_rate_limit_headers("POST /memory/:agent_id/consolidate", r)))

# Clear Memory
r = requests.delete(f"{BASE_URL}/memory/test_agent", headers=headers)
results.append(("Clear Memory", check_rate_limit_headers("DELETE /memory/:agent_id", r)))

print("\n[7] Health Check (should NOT have rate limiting)...")
r = requests.get(f"{BASE_URL}/health")
if r.headers.get("X-RateLimit-Limit"):
    print(f"  WARN: Health check has rate limiting (may be intentional)")
else:
    print(f"  OK: Health check has no rate limiting")

# Summary
print("\n" + "=" * 70)
print("SUMMARY")
print("=" * 70)

passed = sum(1 for _, ok in results if ok)
total = len(results)

print(f"\nPassed: {passed}/{total}")

if passed == total:
    print("\nSUCCESS: All endpoints have rate limiting headers!")
else:
    print("\nFailed endpoints:")
    for name, ok in results:
        if not ok:
            print(f"  - {name}")

print("\nTest completed.")
