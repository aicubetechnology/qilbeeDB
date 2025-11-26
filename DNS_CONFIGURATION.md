# DNS Configuration for docs.qilbeedb.io

This document explains how to configure your DNS to point `docs.qilbeedb.io` to the S3 bucket.

## Current Setup

- **S3 Bucket**: `docs.qilbeedb.io`
- **Region**: `us-east-1`
- **S3 Website Endpoint**: `http://docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
- **Status**: Bucket configured and documentation deployed

## DNS Configuration Options

You have two main options for configuring DNS:

### Option 1: CNAME Record (Recommended for Non-Root Subdomain)

If you're using a DNS provider like Cloudflare, Route 53, or similar:

1. **Go to your DNS management console** for `qilbeedb.io`

2. **Create a CNAME record**:
   - **Name/Host**: `docs`
   - **Type**: `CNAME`
   - **Value/Target**: `docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
   - **TTL**: `300` (5 minutes) or `3600` (1 hour)

Example for common DNS providers:

#### Cloudflare
```
Type: CNAME
Name: docs
Target: docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com
Proxy status: DNS only (disable orange cloud)
TTL: Auto
```

#### AWS Route 53
```
Record name: docs
Record type: CNAME
Value: docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com
Routing policy: Simple routing
TTL: 300
```

#### GoDaddy / Namecheap / Other
```
Type: CNAME
Host: docs
Points to: docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com
TTL: 1 Hour
```

### Option 2: AWS Route 53 Alias Record (Recommended if using Route 53)

If your domain is managed in AWS Route 53:

1. **Open Route 53 Console**
2. **Select your hosted zone** for `qilbeedb.io`
3. **Create record**:
   - **Record name**: `docs`
   - **Record type**: `A` (IPv4 address)
   - **Enable Alias**: Yes
   - **Route traffic to**:
     - Select `Alias to S3 website endpoint`
     - Region: `US East (N. Virginia) us-east-1`
     - S3 bucket: `docs.qilbeedb.io`
   - **Routing policy**: Simple routing
   - **Evaluate target health**: No

## Verification

After configuring DNS, wait for DNS propagation (usually 5-30 minutes) and test:

### 1. Check DNS Resolution

```bash
# Check if CNAME resolves
dig docs.qilbeedb.io

# Check if it points to S3
nslookup docs.qilbeedb.io
```

Expected output should show:
```
docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com
```

### 2. Test in Browser

Open your browser and navigate to:
```
http://docs.qilbeedb.io
```

You should see the QilbeeDB documentation homepage.

### 3. Test with curl

```bash
curl -I http://docs.qilbeedb.io
```

Should return HTTP 200 OK status.

## Important Notes

### HTTP vs HTTPS

**Current Setup**: HTTP only (port 80)

The S3 static website endpoint only supports HTTP. For HTTPS support, you need to:

1. **Use CloudFront** (AWS CDN):
   - Create a CloudFront distribution
   - Point it to your S3 bucket
   - Add SSL certificate from AWS Certificate Manager (ACM)
   - Update DNS to point to CloudFront instead

2. **Use Cloudflare** (if using Cloudflare DNS):
   - Keep CNAME pointing to S3 endpoint
   - Enable "Flexible SSL" in Cloudflare (free)
   - Cloudflare will handle HTTPS for visitors
   - Connection between Cloudflare and S3 will be HTTP

### Recommended: Enable HTTPS with CloudFront

For production use, HTTPS is recommended. Here's how to set it up:

1. **Request SSL Certificate** (AWS Certificate Manager):
   ```bash
   # Request certificate for docs.qilbeedb.io
   aws acm request-certificate \
     --domain-name docs.qilbeedb.io \
     --validation-method DNS \
     --region us-east-1
   ```

2. **Validate the certificate** by adding the DNS records provided by ACM

3. **Create CloudFront Distribution**:
   ```bash
   aws cloudfront create-distribution \
     --origin-domain-name docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com \
     --default-root-object index.html
   ```

4. **Update DNS** to point to CloudFront distribution instead of S3 directly

## Quick Setup Script

If you want to automate the CloudFront + HTTPS setup, here's a helper script:

```bash
#!/bin/bash

# Variables
DOMAIN="docs.qilbeedb.io"
BUCKET="docs.qilbeedb.io"
REGION="us-east-1"

# 1. Request SSL certificate
echo "Requesting SSL certificate..."
CERT_ARN=$(aws acm request-certificate \
  --domain-name $DOMAIN \
  --validation-method DNS \
  --region us-east-1 \
  --query 'CertificateArn' \
  --output text)

echo "Certificate ARN: $CERT_ARN"
echo "Please add the DNS validation records shown in ACM console"
echo "Waiting for certificate validation..."

# Wait for certificate to be issued
aws acm wait certificate-validated \
  --certificate-arn $CERT_ARN \
  --region us-east-1

# 2. Create CloudFront distribution
echo "Creating CloudFront distribution..."
DIST_ID=$(aws cloudfront create-distribution \
  --origin-domain-name $BUCKET.s3-website-$REGION.amazonaws.com \
  --default-root-object index.html \
  --query 'Distribution.Id' \
  --output text)

echo "CloudFront Distribution ID: $DIST_ID"
echo "Domain name: $(aws cloudfront get-distribution --id $DIST_ID --query 'Distribution.DomainName' --output text)"
```

## Troubleshooting

### DNS not resolving
- Wait up to 48 hours for full DNS propagation
- Clear DNS cache: `sudo dscacheutil -flushcache` (Mac) or `ipconfig /flushdns` (Windows)
- Use `dig +trace docs.qilbeedb.io` to see propagation path

### 403 Forbidden error
- Check S3 bucket policy allows public read access
- Verify bucket name exactly matches domain name
- Confirm static website hosting is enabled

### 404 Not Found
- Verify files were uploaded correctly
- Check that `index.html` exists in bucket root
- Confirm error document is set to `404.html`

## Current Deployment Commands

To deploy updated documentation to the new bucket:

```bash
# Build documentation
source docs-venv/bin/activate
mkdocs build

# Deploy to S3 with correct content types
cd site

# Upload HTML files
AWS_PROFILE=aicube-bruno-noprod aws s3 sync . s3://docs.qilbeedb.io \
  --exclude "*" --include "*.html" \
  --content-type "text/html" \
  --cache-control "public, max-age=300"

# Upload CSS files
AWS_PROFILE=aicube-bruno-noprod aws s3 sync . s3://docs.qilbeedb.io \
  --exclude "*" --include "*.css" \
  --content-type "text/css" \
  --cache-control "public, max-age=31536000"

# Upload JS files
AWS_PROFILE=aicube-bruno-noprod aws s3 sync . s3://docs.qilbeedb.io \
  --exclude "*" --include "*.js" \
  --content-type "application/javascript" \
  --cache-control "public, max-age=31536000"

# Upload remaining files
AWS_PROFILE=aicube-bruno-noprod aws s3 sync . s3://docs.qilbeedb.io \
  --exclude "*.html" --exclude "*.css" --exclude "*.js"

cd ..
```

## Summary

**To get `docs.qilbeedb.io` working:**

1. Add CNAME DNS record pointing `docs` to `docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
2. Wait for DNS propagation (5-30 minutes)
3. Test: `http://docs.qilbeedb.io`
4. (Optional) Set up CloudFront + SSL for HTTPS support

**Current Status:**
- S3 bucket `docs.qilbeedb.io` created and configured
- Static website hosting enabled
- Documentation files deployed
- Public read access configured
- DNS configuration needed (your action)

Once DNS is configured, the documentation will be accessible at:
- HTTP: `http://docs.qilbeedb.io`
- (After CloudFront setup): `https://docs.qilbeedb.io`
