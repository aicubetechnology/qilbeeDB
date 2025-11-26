#!/bin/bash

# DNS Propagation Monitor for docs.qilbeedb.io
# This script checks every 60 seconds if DNS has updated to CloudFront

EXPECTED_CNAME="d3f4edmb6vok7h.cloudfront.net."
DOMAIN="docs.qilbeedb.io"
CHECK_INTERVAL=60  # seconds
MAX_CHECKS=60      # Check for up to 60 minutes

echo "=============================================="
echo "DNS Propagation Monitor"
echo "=============================================="
echo "Domain: $DOMAIN"
echo "Expected CNAME: $EXPECTED_CNAME"
echo "Checking every $CHECK_INTERVAL seconds..."
echo "Max wait time: $((MAX_CHECKS * CHECK_INTERVAL / 60)) minutes"
echo ""
echo "Press Ctrl+C to stop monitoring"
echo "=============================================="
echo ""

COUNT=0
START_TIME=$(date +%s)

while [ $COUNT -lt $MAX_CHECKS ]; do
  COUNT=$((COUNT + 1))
  CURRENT_TIME=$(date +%s)
  ELAPSED=$((CURRENT_TIME - START_TIME))
  ELAPSED_MIN=$((ELAPSED / 60))
  ELAPSED_SEC=$((ELAPSED % 60))

  echo "[$COUNT/$MAX_CHECKS] Checking DNS... (Elapsed: ${ELAPSED_MIN}m ${ELAPSED_SEC}s)"

  # Get current CNAME
  CURRENT_CNAME=$(dig $DOMAIN CNAME +short 2>/dev/null)

  if [ -z "$CURRENT_CNAME" ]; then
    echo "  ⚠️  No CNAME record found (DNS query failed or no record)"
  elif echo "$CURRENT_CNAME" | grep -q "cloudfront.net"; then
    echo ""
    echo "=============================================="
    echo "✓ DNS UPDATED SUCCESSFULLY!"
    echo "=============================================="
    echo "Current CNAME: $CURRENT_CNAME"
    echo "Time elapsed: ${ELAPSED_MIN} minutes ${ELAPSED_SEC} seconds"
    echo ""
    echo "Now testing HTTPS access..."
    echo ""

    # Test HTTPS
    echo "Testing: https://$DOMAIN"
    HTTP_STATUS=$(curl -I -s -o /dev/null -w "%{http_code}" https://$DOMAIN 2>/dev/null)

    if [ "$HTTP_STATUS" = "200" ]; then
      echo "✓ HTTPS working! Status: $HTTP_STATUS"
    else
      echo "⚠️  HTTPS returned status: $HTTP_STATUS (may need a few more minutes)"
    fi

    echo ""
    echo "Testing HTTP → HTTPS redirect..."
    HTTP_REDIRECT=$(curl -I -s http://$DOMAIN 2>/dev/null | grep -i "location:" | head -1)

    if echo "$HTTP_REDIRECT" | grep -q "https://"; then
      echo "✓ HTTP redirects to HTTPS"
      echo "  $HTTP_REDIRECT"
    else
      echo "⚠️  HTTP redirect status unknown"
    fi

    echo ""
    echo "=============================================="
    echo "Setup complete! Your site is live at:"
    echo "  https://$DOMAIN"
    echo "=============================================="
    exit 0
  else
    echo "  Current: $CURRENT_CNAME"
    echo "  Status: Still pointing to S3 (not CloudFront yet)"
  fi

  if [ $COUNT -lt $MAX_CHECKS ]; then
    echo "  Waiting $CHECK_INTERVAL seconds before next check..."
    echo ""
    sleep $CHECK_INTERVAL
  fi
done

echo ""
echo "=============================================="
echo "Timeout: DNS still not updated after $((MAX_CHECKS * CHECK_INTERVAL / 60)) minutes"
echo "=============================================="
echo "Current CNAME: $(dig $DOMAIN CNAME +short)"
echo ""
echo "This can happen if:"
echo "1. DNS changes take longer than usual (can be up to 48 hours)"
echo "2. The change wasn't saved properly in GoDaddy"
echo "3. There's a typo in the CloudFront domain"
echo ""
echo "Please verify in GoDaddy that the 'docs' CNAME shows:"
echo "  $EXPECTED_CNAME"
echo ""
echo "You can run this script again later: ./monitor-dns.sh"
echo "=============================================="
exit 1
