#!/bin/bash

echo "Testing rate limit enforcement on login endpoint"
echo "================================================"

for i in $(seq 1 10); do
  response=$(curl -s -D - -X POST http://localhost:7474/api/v1/auth/login \
    -H "Content-Type: application/json" \
    -d '{"username":"testuser","password":"wrongpass"}' 2>&1)

  status=$(echo "$response" | grep "^HTTP" | head -1 | awk '{print $2}')
  remaining=$(echo "$response" | grep -i "x-ratelimit-remaining" | awk '{print $2}' | tr -d '\r\n')
  limit=$(echo "$response" | grep -i "x-ratelimit-limit" | awk '{print $2}' | tr -d '\r\n')

  echo "Request $i: HTTP $status | Limit: $limit | Remaining: $remaining"
done

echo ""
echo "If Remaining decreased with each request, rate limiting is working."
echo "If HTTP status became 429, rate limit was enforced."
