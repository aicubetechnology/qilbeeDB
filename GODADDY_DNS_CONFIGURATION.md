# GoDaddy DNS Configuration for docs.qilbeedb.io

## Current Status

CloudFront Distribution is working perfectly!
- Direct access works: https://d3f4edmb6vok7h.cloudfront.net
- SSL certificate: Valid
- HTTP/2: Enabled
- Status: Deployed

## Problem

The DNS CNAME record is still pointing to the old S3 endpoint instead of CloudFront.

**Current DNS value:** `docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`
**Correct DNS value:** `d3f4edmb6vok7h.cloudfront.net`

## Exact GoDaddy Configuration

You need **TWO** CNAME records in GoDaddy for `qilbeedb.io`:

### Record 1: Main Documentation Access (THIS ONE NEEDS TO BE UPDATED!)

| Field | Value |
|-------|-------|
| **Type** | CNAME |
| **Name** | `docs` |
| **Value** | `d3f4edmb6vok7h.cloudfront.net` |
| **TTL** | 600 seconds (or 1 hour) |

**IMPORTANT:** The value should be EXACTLY `d3f4edmb6vok7h.cloudfront.net`
- No `https://` prefix
- No trailing `/`
- No trailing `.` (GoDaddy adds this automatically)
- Just the CloudFront domain name

### Record 2: SSL Certificate Validation (Keep this one as-is)

| Field | Value |
|-------|-------|
| **Type** | CNAME |
| **Name** | `_3f21887fd2fd548a850ef4616ba4b488.docs` |
| **Value** | `_489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws.` |
| **TTL** | 600 seconds (or 1 hour) |

**Note:** This validation record should already be there. Don't remove it - AWS needs it for automatic certificate renewal.

## Step-by-Step in GoDaddy

1. **Log into GoDaddy** (https://dcc.godaddy.com/control/dns)

2. **Find your domain**:
   - Go to "My Products"
   - Find `qilbeedb.io`
   - Click "DNS" or "Manage DNS"

3. **Locate the `docs` CNAME record**:
   - Look for a record with Name = `docs`
   - Type = `CNAME`
   - Current value is probably: `docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com`

4. **Edit the record**:
   - Click the pencil/edit icon next to the `docs` record
   - Change the **Value** field to: `d3f4edmb6vok7h.cloudfront.net`
   - TTL: 600 (or 1 Hour)
   - Click **Save**

5. **Verify you saved it**:
   - The record should now show:
     - Name: `docs`
     - Type: `CNAME`
     - Value: `d3f4edmb6vok7h.cloudfront.net`

## What It Should Look Like

Your DNS records page should show something like this:

```
Type    Name                                    Value                                                           TTL
----    ----                                    -----                                                           ---
CNAME   docs                                    d3f4edmb6vok7h.cloudfront.net                                  1 Hour
CNAME   _3f21887fd2fd548a850ef4616ba4b488.docs _489416868a7cd4eb454438da657fe7c9.jkddzztszm.acm-validations.aws. 1 Hour
```

## After Updating

1. **Wait 5-30 minutes** for DNS propagation
2. **Clear your browser cache** (or use incognito mode)
3. **Test the URL**: https://docs.qilbeedb.io

## Verification Commands

After updating and waiting a few minutes, run these commands to verify:

```bash
# Check if DNS updated (should show CloudFront IPs, not S3)
dig docs.qilbeedb.io +short

# Check CNAME record (should show d3f4edmb6vok7h.cloudfront.net)
dig docs.qilbeedb.io CNAME +short

# Test HTTPS (should return HTTP/2 200)
curl -I https://docs.qilbeedb.io

# Test HTTP redirect (should redirect to HTTPS)
curl -I http://docs.qilbeedb.io
```

## Common Mistakes to Avoid

1. **Don't include "https://"** in the value field
   - Wrong: `https://d3f4edmb6vok7h.cloudfront.net`
   - Correct: `d3f4edmb6vok7h.cloudfront.net`

2. **Don't add a trailing slash**
   - Wrong: `d3f4edmb6vok7h.cloudfront.net/`
   - Correct: `d3f4edmb6vok7h.cloudfront.net`

3. **Don't confuse it with the validation record**
   - You need BOTH records
   - The `docs` record is for traffic
   - The `_3f21887fd2fd548a850ef4616ba4b488.docs` record is for SSL validation

4. **Make sure you clicked Save**
   - Some GoDaddy interfaces require two saves (edit + save at bottom of page)

## Troubleshooting

### If it's still not working after 30 minutes:

1. **Verify the record in GoDaddy**:
   - Log back into GoDaddy DNS management
   - Confirm the `docs` CNAME shows `d3f4edmb6vok7h.cloudfront.net`

2. **Check for typos**:
   - The CloudFront domain must be EXACTLY: `d3f4edmb6vok7h.cloudfront.net`
   - Character-by-character: `d-3-f-4-e-d-m-b-6-v-o-k-7-h.cloudfront.net`

3. **Clear DNS cache locally**:
   ```bash
   # Mac
   sudo dscacheutil -flushcache

   # Windows
   ipconfig /flushdns
   ```

4. **Wait longer**:
   - DNS propagation can sometimes take up to 48 hours
   - But 90% of the time it's within 30 minutes

## Screenshot Guide

When you're in GoDaddy DNS management, you should see:

```
[Edit] docs    CNAME    d3f4edmb6vok7h.cloudfront.net    1 Hour
```

The edit form should have:
- Type: CNAME (dropdown, don't change)
- Name: docs (don't change)
- Value: d3f4edmb6vok7h.cloudfront.net (THIS is what you need to update)
- TTL: 600 or 1 Hour

## Quick Copy-Paste Values

For easy copy-paste into GoDaddy:

**Record Name:** `docs`
**Record Type:** `CNAME`
**Record Value:** `d3f4edmb6vok7h.cloudfront.net`

## Current Verification

As of now, the DNS still shows:
```
$ dig docs.qilbeedb.io CNAME +short
docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com.
```

After your update, it should show:
```
$ dig docs.qilbeedb.io CNAME +short
d3f4edmb6vok7h.cloudfront.net.
```

## Summary

**Change this ONE record in GoDaddy:**

From:
```
Name: docs
Value: docs.qilbeedb.io.s3-website-us-east-1.amazonaws.com
```

To:
```
Name: docs
Value: d3f4edmb6vok7h.cloudfront.net
```

Then wait 5-30 minutes for DNS propagation, and https://docs.qilbeedb.io will work with CloudFront + SSL!
