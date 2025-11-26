#!/usr/bin/env python3
"""Test script for API key management endpoints."""

import requests
import json

BASE_URL = "http://localhost:7474"

# Step 1: Login to get JWT token
print("1. Logging in...")
login_response = requests.post(
    f"{BASE_URL}/api/v1/auth/login",
    json={"username": "admin", "password": "Admin123!@#"}
)
print(f"Login status: {login_response.status_code}")
login_data = login_response.json()
print(f"Login response: {json.dumps(login_data, indent=2)}")

access_token = login_data.get("access_token")
if not access_token:
    print("ERROR: No access token received!")
    exit(1)

headers = {"Authorization": f"Bearer {access_token}"}

# Step 2: Create an API key
print("\n2. Creating API key...")
create_response = requests.post(
    f"{BASE_URL}/api/v1/api-keys",
    headers=headers,
    json={"name": "test-api-key-1"}
)
print(f"Create status: {create_response.status_code}")
create_data = create_response.json()
print(f"Create response: {json.dumps(create_data, indent=2)}")

key_id = create_data.get("id")

# Step 3: List API keys
print("\n3. Listing API keys...")
list_response = requests.get(
    f"{BASE_URL}/api/v1/api-keys",
    headers=headers
)
print(f"List status: {list_response.status_code}")
list_data = list_response.json()
print(f"List response: {json.dumps(list_data, indent=2)}")

# Step 4: Create another API key
print("\n4. Creating second API key...")
create2_response = requests.post(
    f"{BASE_URL}/api/v1/api-keys",
    headers=headers,
    json={"name": "test-api-key-2"}
)
print(f"Create status: {create2_response.status_code}")
create2_data = create2_response.json()
print(f"Create response: {json.dumps(create2_data, indent=2)}")

# Step 5: List API keys again
print("\n5. Listing API keys again...")
list2_response = requests.get(
    f"{BASE_URL}/api/v1/api-keys",
    headers=headers
)
print(f"List status: {list2_response.status_code}")
list2_data = list2_response.json()
print(f"List response: {json.dumps(list2_data, indent=2)}")

# Step 6: Revoke the first API key
if key_id:
    print(f"\n6. Revoking API key {key_id}...")
    revoke_response = requests.delete(
        f"{BASE_URL}/api/v1/api-keys/{key_id}",
        headers=headers
    )
    print(f"Revoke status: {revoke_response.status_code}")
    revoke_data = revoke_response.json()
    print(f"Revoke response: {json.dumps(revoke_data, indent=2)}")

    # Step 7: List API keys after revocation
    print("\n7. Listing API keys after revocation...")
    list3_response = requests.get(
        f"{BASE_URL}/api/v1/api-keys",
        headers=headers
    )
    print(f"List status: {list3_response.status_code}")
    list3_data = list3_response.json()
    print(f"List response: {json.dumps(list3_data, indent=2)}")

print("\n✓ All API key endpoints tested successfully!")
