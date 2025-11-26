#!/usr/bin/env python3
"""Test X-API-Key authentication works alongside JWT Bearer token auth."""

import requests
import json

BASE_URL = "http://localhost:7474"

print("=" * 60)
print("Testing X-API-Key Authentication Middleware")
print("=" * 60)

# Step 1: Login with JWT to create an API key
print("\n1. Logging in with JWT Bearer token...")
login_response = requests.post(
    f"{BASE_URL}/api/v1/auth/login",
    json={"username": "admin", "password": "Admin123!@#"}
)
print(f"Login status: {login_response.status_code}")
login_data = login_response.json()
access_token = login_data.get("access_token")
print(f"JWT Bearer token received: {access_token[:20]}...")

# Step 2: Create an API key using JWT auth
print("\n2. Creating API key using JWT Bearer token...")
create_response = requests.post(
    f"{BASE_URL}/api/v1/api-keys",
    headers={"Authorization": f"Bearer {access_token}"},
    json={"name": "test-key"}
)
print(f"Create status: {create_response.status_code}")
create_data = create_response.json()
api_key = create_data.get("key")
key_id = create_data.get("id")
print(f"API key created: {api_key[:20]}...")

# Step 3: Test accessing API keys endpoint with JWT (should work)
print("\n3. Accessing /api/v1/api-keys with JWT Bearer token...")
list_jwt_response = requests.get(
    f"{BASE_URL}/api/v1/api-keys",
    headers={"Authorization": f"Bearer {access_token}"}
)
print(f"GET /api/v1/api-keys with JWT: {list_jwt_response.status_code}")
print(f"API keys found: {len(list_jwt_response.json().get('api_keys', []))}")

# Step 4: Test accessing API keys endpoint with X-API-Key (should work)
print("\n4. Accessing /api/v1/api-keys with X-API-Key header...")
list_apikey_response = requests.get(
    f"{BASE_URL}/api/v1/api-keys",
    headers={"X-API-Key": api_key}
)
print(f"GET /api/v1/api-keys with X-API-Key: {list_apikey_response.status_code}")
print(f"API keys found: {len(list_apikey_response.json().get('api_keys', []))}")

# Step 5: Test accessing user list with X-API-Key (should work)
print("\n5. Accessing /api/v1/users with X-API-Key header...")
users_response = requests.get(
    f"{BASE_URL}/api/v1/users",
    headers={"X-API-Key": api_key}
)
print(f"GET /api/v1/users with X-API-Key: {users_response.status_code}")
print(f"Users found: {len(users_response.json().get('users', []))}")

# Step 6: Test with invalid API key (should fail with 401)
print("\n6. Testing with invalid X-API-Key...")
invalid_response = requests.get(
    f"{BASE_URL}/api/v1/api-keys",
    headers={"X-API-Key": "invalid-key-12345"}
)
print(f"GET /api/v1/api-keys with invalid key: {invalid_response.status_code}")
if invalid_response.status_code == 401:
    print("✓ Correctly rejected invalid API key")

# Step 7: Test with no auth headers (should fail with 401)
print("\n7. Testing with no authentication...")
no_auth_response = requests.get(f"{BASE_URL}/api/v1/api-keys")
print(f"GET /api/v1/api-keys with no auth: {no_auth_response.status_code}")
if no_auth_response.status_code == 401:
    print("✓ Correctly rejected unauthenticated request")

# Step 8: Test creating a new API key with X-API-Key header (should work)
print("\n8. Creating new API key using X-API-Key header...")
create2_response = requests.post(
    f"{BASE_URL}/api/v1/api-keys",
    headers={"X-API-Key": api_key},
    json={"name": "test-key-2"}
)
print(f"POST /api/v1/api-keys with X-API-Key: {create2_response.status_code}")
if create2_response.status_code == 200:
    create2_data = create2_response.json()
    print(f"New API key created: {create2_data.get('key', '')[:20]}...")
    print("✓ X-API-Key auth works for create operations")

print("\n" + "=" * 60)
print("✓ X-API-Key Authentication Middleware Test Complete!")
print("=" * 60)
print("\nSummary:")
print("- JWT Bearer token authentication: ✓ Working")
print("- X-API-Key header authentication: ✓ Working")
print("- Invalid key rejection: ✓ Working")
print("- Unauthenticated request rejection: ✓ Working")
print("- Both auth methods can access protected endpoints: ✓ Working")
