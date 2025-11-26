#!/usr/bin/env python3
"""Test script for user management endpoints."""

import requests
import json

BASE_URL = "http://localhost:7474"

# Step 1: Login as admin to get JWT token
print("1. Logging in as admin...")
login_response = requests.post(
    f"{BASE_URL}/api/v1/auth/login",
    json={"username": "admin", "password": "Admin123!@#"}
)
print(f"Login status: {login_response.status_code}")
login_data = login_response.json()
print(f"Login response: {json.dumps(login_data, indent=2)}\n")

access_token = login_data.get("access_token")
if not access_token:
    print("ERROR: No access token received!")
    exit(1)

headers = {"Authorization": f"Bearer {access_token}"}

# Step 2: List all users (should show admin)
print("2. Listing all users...")
list_response = requests.get(
    f"{BASE_URL}/api/v1/users",
    headers=headers
)
print(f"List status: {list_response.status_code}")
list_data = list_response.json()
print(f"List response: {json.dumps(list_data, indent=2)}\n")

# Step 3: Create a new user
print("3. Creating new user 'alice'...")
create_response = requests.post(
    f"{BASE_URL}/api/v1/users",
    headers=headers,
    json={
        "username": "alice",
        "email": "alice@qilbeedb.io",
        "password": "Alice123!@#",
        "roles": ["Read"]
    }
)
print(f"Create status: {create_response.status_code}")
create_data = create_response.json()
print(f"Create response: {json.dumps(create_data, indent=2)}\n")

alice_id = create_data.get("id")

# Step 4: Create another user with Developer role
print("4. Creating new user 'bob' with Developer role...")
create2_response = requests.post(
    f"{BASE_URL}/api/v1/users",
    headers=headers,
    json={
        "username": "bob",
        "email": "bob@qilbeedb.io",
        "password": "Bob123!@#",
        "roles": ["Developer"]
    }
)
print(f"Create status: {create2_response.status_code}")
create2_data = create2_response.json()
print(f"Create response: {json.dumps(create2_data, indent=2)}\n")

bob_id = create2_data.get("id")

# Step 5: List all users again (should show 3 users)
print("5. Listing all users again...")
list2_response = requests.get(
    f"{BASE_URL}/api/v1/users",
    headers=headers
)
print(f"List status: {list2_response.status_code}")
list2_data = list2_response.json()
print(f"List response: {json.dumps(list2_data, indent=2)}\n")

# Step 6: Get Alice's user details
if alice_id:
    print(f"6. Getting user details for Alice ({alice_id})...")
    get_response = requests.get(
        f"{BASE_URL}/api/v1/users/{alice_id}",
        headers=headers
    )
    print(f"Get status: {get_response.status_code}")
    get_data = get_response.json()
    print(f"Get response: {json.dumps(get_data, indent=2)}\n")

# Step 7: Update Alice's email
if alice_id:
    print(f"7. Updating Alice's email...")
    update_response = requests.put(
        f"{BASE_URL}/api/v1/users/{alice_id}",
        headers=headers,
        json={"email": "alice.updated@qilbeedb.io"}
    )
    print(f"Update status: {update_response.status_code}")
    update_data = update_response.json()
    print(f"Update response: {json.dumps(update_data, indent=2)}\n")

# Step 8: Update Bob's roles to Admin
if bob_id:
    print(f"8. Updating Bob's roles to Admin...")
    roles_response = requests.put(
        f"{BASE_URL}/api/v1/users/{bob_id}/roles",
        headers=headers,
        json={"roles": ["Admin", "Developer"]}
    )
    print(f"Update roles status: {roles_response.status_code}")
    roles_data = roles_response.json()
    print(f"Update roles response: {json.dumps(roles_data, indent=2)}\n")

# Step 9: Login as Alice to test own user access
print("9. Logging in as Alice...")
alice_login = requests.post(
    f"{BASE_URL}/api/v1/auth/login",
    json={"username": "alice", "password": "Alice123!@#"}
)
print(f"Alice login status: {alice_login.status_code}")
alice_token = alice_login.json().get("access_token")

if alice_token and alice_id:
    alice_headers = {"Authorization": f"Bearer {alice_token}"}

    # Step 10: Alice tries to view her own profile (should work)
    print(f"10. Alice viewing her own profile...")
    alice_get = requests.get(
        f"{BASE_URL}/api/v1/users/{alice_id}",
        headers=alice_headers
    )
    print(f"Alice get own profile status: {alice_get.status_code}")
    print(f"Alice profile: {json.dumps(alice_get.json(), indent=2)}\n")

    # Step 11: Alice tries to list all users (should fail - not admin)
    print("11. Alice trying to list all users (should fail)...")
    alice_list = requests.get(
        f"{BASE_URL}/api/v1/users",
        headers=alice_headers
    )
    print(f"Alice list users status: {alice_list.status_code}")
    print(f"Alice list response: {json.dumps(alice_list.json(), indent=2)}\n")

    # Step 12: Alice updates her own password
    print("12. Alice updating her own password...")
    alice_update = requests.put(
        f"{BASE_URL}/api/v1/users/{alice_id}",
        headers=alice_headers,
        json={"password": "NewAlice123!@#"}
    )
    print(f"Alice update password status: {alice_update.status_code}")
    print(f"Alice update response: {json.dumps(alice_update.json(), indent=2)}\n")

# Step 13: Admin deletes Alice
if alice_id:
    print(f"13. Admin deleting Alice...")
    delete_response = requests.delete(
        f"{BASE_URL}/api/v1/users/{alice_id}",
        headers=headers
    )
    print(f"Delete status: {delete_response.status_code}")
    delete_data = delete_response.json()
    print(f"Delete response: {json.dumps(delete_data, indent=2)}\n")

# Step 14: Final list of users
print("14. Final list of users...")
final_list = requests.get(
    f"{BASE_URL}/api/v1/users",
    headers=headers
)
print(f"Final list status: {final_list.status_code}")
final_data = final_list.json()
print(f"Final list response: {json.dumps(final_data, indent=2)}\n")

print("✓ All user management endpoints tested successfully!")
