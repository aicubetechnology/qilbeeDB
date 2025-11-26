# CloudFront HTTPS Setup for docs.qilbeedb.io

## Current Status

SSL Certificate requested from AWS Certificate Manager:
- **Certificate ARN**: `arn:aws:acm:us-east-1:225520704248:certificate/7c122e83-c870-4d6b-9ced-e6d86bcb8b79`
- **Domain**: docs.qilbeedb.io
- **Status**: PENDING_VALIDATION

## Step 1: Add DNS Validation Record to GoDaddy

You need to add this CNAME record to validate the SSL certificate:

### DNS Record Details:

| Field | Value |
|-------|-------|
| **Type** | CNAME |
| **Name** | `_3f21887fd2fd548a850ef4616ba4b488.docs` |
| **Value** | `_489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws.` |
| **TTL** | 600 (or Auto) |

### How to Add in GoDaddy:

1. Log into your GoDaddy account
2. Go to **My Products** → **DNS**
3. Find `qilbeedb.io` and click **Manage DNS**
4. Click **Add** button
5. Select **Type**: CNAME
6. **Name**: `_3f21887fd2fd548a850ef4616ba4b488.docs`
7. **Value**: `_489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws.`
8. **TTL**: 600 seconds
9. Click **Save**

**Important**: This is a VALIDATION record - keep it permanent. AWS needs it to renew your certificate automatically.

## Step 2: Wait for Certificate Validation

After adding the DNS record, wait 5-30 minutes for AWS to validate the certificate.

Check validation status:

```bash
AWS_PROFILE=aicube-bruno-noprod aws acm describe-certificate \
  --certificate-arn arn:aws:acm:us-east-1:225520704248:certificate/7c122e83-c870-4d6b-9ced-e6d86bcb8b79 \
  --region us-east-1 \
  --query 'Certificate.Status' \
  --output text
```

When it shows `ISSUED`, proceed to Step 3.

## Step 3: Create CloudFront Distribution

Once the certificate is validated, create the CloudFront distribution:

```bash
AWS_PROFILE=aicube-bruno-noprod aws cloudfront create-distribution \
  --distribution-config file://cloudfront-config.json \
  --region us-east-1
```

This will return a distribution ID and CloudFront domain name (like `d1234567890.cloudfront.net`).

## Step 4: Update DNS CNAME in GoDaddy

After CloudFront is created, **update** your existing `docs` CNAME record:

### Update the `docs` CNAME:

1. Go to GoDaddy DNS management for `qilbeedb.io`
2. Find the existing `docs` CNAME record
3. Click **Edit** (pencil icon)
4. Change **Value** from:
   - Old: `docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
   - New: `[Your CloudFront domain from Step 3].cloudfront.net`
5. Save changes

## Step 5: Wait for DNS Propagation

- Wait 5-30 minutes for DNS to propagate
- CloudFront also takes 15-20 minutes to deploy globally

## Step 6: Verify HTTPS Access

Test your site:

```bash
# Test HTTPS
curl -I https://docs.qilbeedb.io

# Test HTTP redirect to HTTPS
curl -I http://docs.qilbeedb.io
```

Expected results:
- HTTPS should return `200 OK`
- HTTP should return `301` or `307` redirect to HTTPS

## Automated Setup Script

Here's a complete automation script (run after adding validation DNS record):

```bash
#!/bin/bash

# Configuration
CERT_ARN="arn:aws:acm:us-east-1:225520704248:certificate/7c122e83-c870-4d6b-9ced-e6d86bcb8b79"
PROFILE="aicube-bruno-noprod"

echo "Waiting for SSL certificate validation..."
echo "Make sure you've added the DNS validation record in GoDaddy!"
echo ""

# Wait for certificate to be issued
while true; do
  STATUS=$(AWS_PROFILE=$PROFILE aws acm describe-certificate \
    --certificate-arn $CERT_ARN \
    --region us-east-1 \
    --query 'Certificate.Status' \
    --output text)

  if [ "$STATUS" == "ISSUED" ]; then
    echo "Certificate validated successfully!"
    break
  elif [ "$STATUS" == "FAILED" ]; then
    echo "Certificate validation failed!"
    exit 1
  else
    echo "Current status: $STATUS - waiting 30 seconds..."
    sleep 30
  fi
done

# Create CloudFront distribution
echo ""
echo "Creating CloudFront distribution..."
DIST_OUTPUT=$(AWS_PROFILE=$PROFILE aws cloudfront create-distribution \
  --distribution-config file://cloudfront-config.json \
  --region us-east-1)

DIST_ID=$(echo $DIST_OUTPUT | jq -r '.Distribution.Id')
CLOUDFRONT_DOMAIN=$(echo $DIST_OUTPUT | jq -r '.Distribution.DomainName')

echo ""
echo "CloudFront Distribution Created!"
echo "Distribution ID: $DIST_ID"
echo "CloudFront Domain: $CLOUDFRONT_DOMAIN"
echo ""
echo "====================================================================="
echo "IMPORTANT: Update your DNS in GoDaddy"
echo "====================================================================="
echo ""
echo "Edit the 'docs' CNAME record in GoDaddy:"
echo "  Name: docs"
echo "  Type: CNAME"
echo "  Value: $CLOUDFRONT_DOMAIN"
echo ""
echo "Current value to replace: docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com"
echo "New value: $CLOUDFRONT_DOMAIN"
echo ""
echo "After updating DNS, wait 15-30 minutes for:"
echo "  - DNS propagation"
echo "  - CloudFront deployment"
echo ""
echo "Then test: https://docs.qilbeedb.io"
echo "====================================================================="
```

Save this as `setup-cloudfront.sh` and run:

```bash
chmod +x setup-cloudfront.sh
./setup-cloudfront.sh
```

## DNS Records Summary

After completing all steps, you should have these DNS records in GoDaddy:

| Name | Type | Value | Purpose |
|------|------|-------|---------|
| `docs` | CNAME | `[cloudfront-domain].cloudfront.net` | Points to CloudFront |
| `_3f21887fd2fd548a850ef4616ba4b488.docs` | CNAME | `_489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws.` | SSL validation |

## CloudFront Benefits

Once configured, you'll have:

- **HTTPS Support**: Secure connections with AWS SSL certificate
- **HTTP → HTTPS Redirect**: Automatic redirect from HTTP to HTTPS
- **Global CDN**: Fast content delivery worldwide
- **Compression**: Automatic Gzip/Brotli compression
- **Caching**: Reduced load on S3, faster page loads
- **Custom Error Pages**: 404 error handling

## Troubleshooting

### Certificate Validation Stuck

If certificate stays in PENDING_VALIDATION:

1. Check DNS record was added correctly in GoDaddy
2. Make sure the Name field is exactly: `_3f21887fd2fd548a850ef4616ba4b488.docs`
3. Make sure the Value ends with a dot: `...acm-validations.aws.`
4. Wait up to 30 minutes for DNS propagation
5. Check DNS propagation: `dig _3f21887fd2fd548a850ef4616ba4b488.docs.qilbeedb.io`

### CloudFront Shows Error

If you get CloudFront errors:

1. Wait 15-20 minutes for full deployment
2. Check S3 bucket is accessible: `curl http://docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
3. Verify CloudFront distribution status: `aws cloudfront get-distribution --id [DIST_ID]`

### HTTPS Not Working

If HTTPS doesn't work:

1. Verify DNS points to CloudFront (not S3)
2. Check certificate is issued: Status = ISSUED
3. Wait for CloudFront deployment (can take 20-30 minutes)
4. Clear browser cache or try incognito mode

## Manual CloudFront Configuration

If you prefer to use AWS Console instead of CLI:

1. Go to **CloudFront** in AWS Console
2. Click **Create Distribution**
3. **Origin Settings**:
   - Origin Domain: `docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
   - Protocol: HTTP only
   - Name: S3-docs.qilbeedb.io
4. **Default Cache Behavior**:
   - Viewer Protocol Policy: Redirect HTTP to HTTPS
   - Allowed HTTP Methods: GET, HEAD
   - Compress Objects: Yes
5. **Distribution Settings**:
   - Alternate Domain Names (CNAMEs): `docs.qilbeedb.io`
   - SSL Certificate: Custom SSL Certificate
   - Select your certificate: docs.qilbeedb.io
   - Default Root Object: `index.html`
6. **Custom Error Pages**:
   - Add error response: 404 → /404.html (404)
7. Click **Create Distribution**

## Cost Estimate

CloudFront pricing (approximate):
- First 10 TB/month: $0.085/GB
- Data transfer: Usually $50-200/month for documentation sites
- SSL certificate: FREE (via AWS Certificate Manager)
- No minimum commitment

For a documentation site with moderate traffic:
- **Estimated cost**: $5-20/month

## Next Steps

After HTTPS is working:

1. Update GitHub README.md to use https://docs.qilbeedb.io
2. Update all internal documentation links to HTTPS
3. Set up CloudFront cache invalidation for deployments
4. Consider adding CloudFront Functions for redirects/rewrites

## Cache Invalidation

When you update documentation, invalidate CloudFront cache:

```bash
AWS_PROFILE=aicube-bruno-noprod aws cloudfront create-invalidation \
  --distribution-id [YOUR_DIST_ID] \
  --paths "/*"
```

This ensures users see the latest version immediately.

## Current Actions Required

**RIGHT NOW:**

1. Add DNS validation record in GoDaddy (see Step 1)
2. Wait for validation (check with the command in Step 2)
3. Let me know when certificate is ISSUED, and I'll create the CloudFront distribution for you

**The validation DNS record details again:**
- Name: `_3f21887fd2fd548a850ef4616ba4b488.docs`
- Value: `_489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws.`
