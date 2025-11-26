#!/bin/bash

# CloudFront Setup Script for QilbeeDB Documentation
# This script automates the CloudFront distribution creation after SSL certificate validation

set -e

# Configuration
CERT_ARN="arn:aws:acm:us-east-1:225520704248:certificate/7c122e83-c870-4d6b-9ced-e6d86bcb8b79"
PROFILE="aicube-bruno-noprod"
REGION="us-east-1"

echo "============================================================"
echo "CloudFront Setup for docs.qilbeedb.io"
echo "============================================================"
echo ""
echo "Prerequisites:"
echo "  1. DNS validation record added in GoDaddy"
echo "  2. Name: _3f21887fd2fd548a850ef4616ba4b488.docs"
echo "  3. Value: _489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws."
echo ""
read -p "Have you added the DNS validation record? (yes/no): " DNS_ADDED

if [ "$DNS_ADDED" != "yes" ]; then
  echo "Please add the DNS validation record first and run this script again."
  exit 1
fi

echo ""
echo "Step 1: Waiting for SSL certificate validation..."
echo "Certificate ARN: $CERT_ARN"
echo ""

WAIT_COUNT=0
MAX_WAIT=60  # Wait up to 30 minutes (60 x 30 seconds)

while true; do
  STATUS=$(AWS_PROFILE=$PROFILE aws acm describe-certificate \
    --certificate-arn $CERT_ARN \
    --region $REGION \
    --query 'Certificate.Status' \
    --output text 2>/dev/null || echo "ERROR")

  if [ "$STATUS" == "ISSUED" ]; then
    echo ""
    echo "✓ Certificate validated successfully!"
    break
  elif [ "$STATUS" == "FAILED" ]; then
    echo ""
    echo "✗ Certificate validation failed!"
    echo "Please check your DNS records in GoDaddy."
    exit 1
  elif [ "$STATUS" == "ERROR" ]; then
    echo "✗ Error checking certificate status. Please check your AWS credentials."
    exit 1
  else
    WAIT_COUNT=$((WAIT_COUNT + 1))
    if [ $WAIT_COUNT -gt $MAX_WAIT ]; then
      echo ""
      echo "✗ Timeout waiting for certificate validation (30 minutes)"
      echo "Please check your DNS records and try again later."
      exit 1
    fi
    echo -n "."
    sleep 30
  fi
done

# Create CloudFront distribution
echo ""
echo "Step 2: Creating CloudFront distribution..."
echo ""

DIST_OUTPUT=$(AWS_PROFILE=$PROFILE aws cloudfront create-distribution \
  --distribution-config file://cloudfront-config.json \
  --output json 2>&1)

if [ $? -ne 0 ]; then
  echo "✗ Failed to create CloudFront distribution"
  echo "$DIST_OUTPUT"
  exit 1
fi

DIST_ID=$(echo "$DIST_OUTPUT" | jq -r '.Distribution.Id')
CLOUDFRONT_DOMAIN=$(echo "$DIST_OUTPUT" | jq -r '.Distribution.DomainName')

echo "✓ CloudFront distribution created successfully!"
echo ""
echo "Distribution Details:"
echo "  ID: $DIST_ID"
echo "  Domain: $CLOUDFRONT_DOMAIN"
echo "  Status: Deploying (will take 15-30 minutes)"
echo ""

# Save distribution info
cat > cloudfront-info.txt << EOF
CloudFront Distribution Information
====================================
Distribution ID: $DIST_ID
CloudFront Domain: $CLOUDFRONT_DOMAIN
Created: $(date)
Status: Deploying

Certificate ARN: $CERT_ARN
Custom Domain: docs.qilbeedb.io
Origin: docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com
EOF

echo "Distribution info saved to: cloudfront-info.txt"
echo ""
echo "============================================================"
echo "NEXT STEPS - UPDATE DNS IN GODADDY"
echo "============================================================"
echo ""
echo "1. Log into GoDaddy"
echo "2. Go to DNS management for qilbeedb.io"
echo "3. Find the 'docs' CNAME record"
echo "4. Edit it and change the value:"
echo ""
echo "   OLD VALUE:"
echo "   docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com"
echo ""
echo "   NEW VALUE:"
echo "   $CLOUDFRONT_DOMAIN"
echo ""
echo "5. Save the changes"
echo ""
echo "============================================================"
echo "VERIFICATION"
echo "============================================================"
echo ""
echo "After updating DNS and waiting 20-30 minutes:"
echo ""
echo "  Test HTTPS:"
echo "  curl -I https://docs.qilbeedb.io"
echo ""
echo "  Test HTTP redirect:"
echo "  curl -I http://docs.qilbeedb.io"
echo ""
echo "  Check CloudFront status:"
echo "  AWS_PROFILE=$PROFILE aws cloudfront get-distribution \\"
echo "    --id $DIST_ID \\"
echo "    --query 'Distribution.Status' \\"
echo "    --output text"
echo ""
echo "============================================================"
echo "Setup script completed!"
echo "============================================================"
