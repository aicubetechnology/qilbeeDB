# QilbeeDB Documentation Deployment

## Deployment Information

### Live Documentation Site
**URL:** http://qilbeedb-docs.s3-website-us-east-1.amazonaws.com

### AWS S3 Configuration
- **Bucket Name:** qilbeedb-docs
- **Region:** us-east-1
- **Static Website Hosting:** Enabled
- **Index Document:** index.html
- **Error Document:** 404.html

### Cache Configuration
- **HTML Files:** 5 minutes (`max-age=300`)
- **Static Assets:** 1 year (`max-age=31536000, immutable`)
- **Other Files:** 1 hour (`max-age=3600`)

## Deployment Process

### Prerequisites
- AWS CLI installed: `brew install awscli`
- AWS credentials configured in `~/.aws/credentials`
- AWS profile: `aicube-bruno-noprod`

### Manual Deployment

To deploy updated documentation:

```bash
# 1. Build the documentation
source docs-venv/bin/activate
mkdocs build
deactivate

# 2. Run the deployment script
./deploy-docs-to-s3.sh
```

### Deployment Script Features

The `deploy-docs-to-s3.sh` script automatically:
- Verifies AWS CLI installation and credentials
- Builds the MkDocs documentation
- Creates S3 bucket if it doesn't exist
- Configures static website hosting
- Disables Block Public Access
- Sets public read bucket policy
- Syncs files with appropriate cache headers
- Supports CloudFront invalidation (optional)

### Environment Variables

You can customize the deployment with these environment variables:

```bash
# Bucket name (default: qilbeedb-docs)
export S3_BUCKET="your-bucket-name"

# AWS region (default: us-east-1)
export AWS_REGION="your-region"

# CloudFront distribution ID (optional)
export CLOUDFRONT_DISTRIBUTION_ID="your-distribution-id"

# AWS profile (default: aicube-bruno-noprod)
export AWS_PROFILE="your-profile"
```

## Documentation Structure

The documentation is built with MkDocs Material and includes:

### Main Sections
- **Getting Started** - Installation, quickstart, configuration
- **Cypher Query Language** - Complete Cypher reference
- **Graph Operations** - Nodes, relationships, properties, indexes, transactions
- **Agent Memory** - Episodes, memory types, consolidation, forgetting
- **Client Libraries** - Python SDK, connections
- **Use Cases** - AI agents, knowledge graphs, social networks, recommendations, multi-agent
- **Architecture** - Overview, storage, query engine, memory engine, bi-temporal
- **API Reference** - HTTP REST API, Bolt protocol, Graph API, Memory API
- **Operations** - Deployment, Docker, monitoring, backup, performance
- **Contributing** - Development setup, code style, testing

### Total Pages
- 52 documentation pages
- Full-text search enabled
- Mobile responsive
- Code syntax highlighting
- Navigation tabs and sections

## Next Steps (Optional)

### HTTPS with CloudFront

For production use with HTTPS:

1. Create CloudFront distribution
2. Point to S3 website endpoint
3. Configure SSL certificate
4. Update DNS records
5. Add CloudFront distribution ID to deployment script

### Custom Domain

To use a custom domain:

1. Register domain in Route 53
2. Create CloudFront distribution with custom domain
3. Add SSL certificate from ACM
4. Update DNS CNAME/ALIAS records

### Continuous Deployment

For automated deployments:

1. Set up GitHub Actions workflow
2. Configure AWS credentials as secrets
3. Trigger on push to main branch
4. Run `mkdocs build` and deployment script

## Monitoring

### Website Status
Check if the site is accessible:
```bash
curl -I http://qilbeedb-docs.s3-website-us-east-1.amazonaws.com
```

### Bucket Information
View bucket configuration:
```bash
aws s3api get-bucket-website --bucket qilbeedb-docs
```

### File Count
Count deployed files:
```bash
aws s3 ls s3://qilbeedb-docs --recursive | wc -l
```

## Troubleshooting

### 403 Forbidden Errors
- Verify Block Public Access is disabled
- Check bucket policy allows public read
- Ensure files have proper permissions

### 404 Not Found Errors
- Verify index.html exists in bucket
- Check static website hosting is enabled
- Confirm correct URL format

### Stale Content
- Clear browser cache
- Wait for cache expiration
- Invalidate CloudFront cache if using CDN

## Deployment History

**Initial Deployment:** 2025-11-24
- Deployed complete MkDocs documentation
- Configured S3 static website hosting
- Set up cache headers
- Total files deployed: 99
- Total size: 6.8 MB

---

For questions or issues, see the deployment script at `deploy-docs-to-s3.sh`
