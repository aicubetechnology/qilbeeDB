# Publishing QilbeeDB Python SDK to PyPI

## Current Status

The Python SDK package has been successfully built and validated:

- **Package name:** `qilbeedb`
- **Version:** `0.1.0`
- **Built distributions:**
  - Source distribution: `qilbeedb-0.1.0.tar.gz` (17KB)
  - Wheel distribution: `qilbeedb-0.1.0-py3-none-any.whl` (15KB)
- **Validation:** All packages passed `twine check` ✓

## Prerequisites

1. **PyPI Account**
   - Create account at: https://pypi.org/account/register/
   - Verify your email address

2. **API Token** (Recommended - more secure than password)
   - Go to: https://pypi.org/manage/account/token/
   - Click "Add API token"
   - Token name: `qilbeedb-upload`
   - Scope: "Entire account" or "Project: qilbeedb" (after first upload)
   - Copy and save the token (starts with `pypi-`)

3. **Configure credentials** (optional but recommended)
   Create or edit `~/.pypirc`:
   ```ini
   [pypi]
   username = __token__
   password = pypi-YOUR_TOKEN_HERE
   ```

## Publishing to PyPI

### Option 1: Using API Token (Recommended)

```bash
cd /Users/kimera/projects/qilbeeDB/sdks/python
source venv/bin/activate

# Upload to PyPI
twine upload dist/* --username __token__ --password pypi-YOUR_TOKEN_HERE
```

### Option 2: Using ~/.pypirc

If you configured `~/.pypirc` with your token:

```bash
cd /Users/kimera/projects/qilbeeDB/sdks/python
source venv/bin/activate

# Upload to PyPI
twine upload dist/*
```

### Option 3: Interactive (will prompt for credentials)

```bash
cd /Users/kimera/projects/qilbeeDB/sdks/python
source venv/bin/activate

# Upload to PyPI (will prompt for username and password/token)
twine upload dist/*
```

## Test PyPI (Optional - Test Before Publishing)

It's recommended to test upload to TestPyPI first:

1. **Create TestPyPI account:** https://test.pypi.org/account/register/
2. **Get TestPyPI token:** https://test.pypi.org/manage/account/token/
3. **Upload to TestPyPI:**

```bash
twine upload --repository testpypi dist/* \
  --username __token__ \
  --password pypi-YOUR_TESTPYPI_TOKEN
```

4. **Test installation:**

```bash
pip install --index-url https://test.pypi.org/simple/ qilbeedb
```

5. **If successful, proceed with real PyPI upload**

## After Publishing

### Verify Publication

1. **Check PyPI page:** https://pypi.org/project/qilbeedb/
2. **Test installation:**

```bash
# Create a new virtual environment
python3 -m venv test-env
source test-env/bin/activate

# Install from PyPI
pip install qilbeedb

# Test import
python -c "from qilbeedb import QilbeeDB; print('Success!')"
```

3. **Check package metadata:**
```bash
pip show qilbeedb
```

### Update Documentation

After publishing, update these locations:

1. **Main README.md** - Add installation instructions
2. **Documentation site** (docs.qilbeedb.io) - Update getting started guide
3. **GitHub README** - Add PyPI badge:

```markdown
[![PyPI version](https://badge.fury.io/py/qilbeedb.svg)](https://badge.fury.io/py/qilbeedb)
[![Downloads](https://pepy.tech/badge/qilbeedb)](https://pepy.tech/project/qilbeedb)
```

## Publishing Updates

When publishing new versions:

1. **Update version** in `setup.py` and `pyproject.toml`
2. **Update CHANGELOG.md** with release notes
3. **Clean old builds:**
   ```bash
   rm -rf dist/ build/ *.egg-info
   ```
4. **Build new distribution:**
   ```bash
   source venv/bin/activate
   python -m build
   ```
5. **Verify:**
   ```bash
   twine check dist/*
   ```
6. **Upload:**
   ```bash
   twine upload dist/*
   ```

## Version Management

Current version: `0.1.0`

Follow semantic versioning (semver):
- **Patch** (0.1.X): Bug fixes, no API changes
- **Minor** (0.X.0): New features, backward compatible
- **Major** (X.0.0): Breaking changes

Example version updates:
- Bug fix: `0.1.0` → `0.1.1`
- New feature: `0.1.0` → `0.2.0`
- Breaking change: `0.1.0` → `1.0.0`

## Troubleshooting

### Error: "File already exists"

This means the version is already published. You need to:
1. Increment the version number in `setup.py` and `pyproject.toml`
2. Rebuild: `python -m build`
3. Upload again

### Error: "Invalid or non-existent authentication"

- Verify your API token is correct
- Make sure username is `__token__` (not your PyPI username)
- Check token hasn't expired

### Error: "Package name already taken"

The package name `qilbeedb` might be taken. Options:
1. Request transfer if it's unused
2. Use alternative name (e.g., `qilbee-db`, `qilbeedb-sdk`)

## Package Maintenance

### Regular Updates

1. **Security updates:** Update dependencies regularly
2. **Python version support:** Test with new Python versions
3. **Documentation:** Keep docs in sync with code
4. **Issue responses:** Monitor GitHub issues

### CI/CD (Future)

Consider setting up GitHub Actions to:
- Run tests on push
- Auto-publish to PyPI on git tag
- Build docs automatically

Example workflow for auto-publishing:
```yaml
name: Publish to PyPI

on:
  push:
    tags:
      - 'v*'

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-python@v4
        with:
          python-version: '3.11'
      - run: pip install build twine
      - run: python -m build
      - run: twine check dist/*
      - run: twine upload dist/*
        env:
          TWINE_USERNAME: __token__
          TWINE_PASSWORD: ${{ secrets.PYPI_TOKEN }}
```

## Security Best Practices

1. **Never commit credentials** to git
2. **Use API tokens** instead of passwords
3. **Limit token scope** to specific projects when possible
4. **Rotate tokens** periodically
5. **Enable 2FA** on PyPI account
6. **Scan for vulnerabilities:**
   ```bash
   pip install safety
   safety check
   ```

## Current Package Info

**Package Details:**
- Name: `qilbeedb`
- Version: `0.1.0`
- Description: Python SDK for QilbeeDB - Enterprise Graph Database with Agent Memory
- Author: QilbeeDB Team
- License: Apache 2.0
- Homepage: https://github.com/aicubetechnology/qilbeeDB
- Documentation: https://docs.qilbeedb.io

**Installation (after publishing):**
```bash
pip install qilbeedb
```

**Quick Start (after publishing):**
```python
from qilbeedb import QilbeeDB

# Connect to database
db = QilbeeDB("http://localhost:7474")

# Get or create a graph
graph = db.graph("mydata")

# Create a node
node = graph.create_node(
    labels=["Person"],
    properties={"name": "Alice", "age": 30}
)
```

## Next Steps

1. **Publish to PyPI** using one of the methods above
2. **Verify installation** works correctly
3. **Update documentation** with installation instructions
4. **Announce release** on GitHub, social media, etc.
5. **Monitor feedback** and respond to issues

## Publishing Command Summary

```bash
# Navigate to SDK directory
cd /Users/kimera/projects/qilbeeDB/sdks/python

# Activate virtual environment
source venv/bin/activate

# Verify package
twine check dist/*

# Upload to PyPI (replace with your token)
twine upload dist/* --username __token__ --password pypi-YOUR_TOKEN_HERE

# Or if ~/.pypirc is configured
twine upload dist/*
```

## Support

For issues or questions:
- GitHub Issues: https://github.com/aicubetechnology/qilbeeDB/issues
- Email: contact@aicube.ca
- Documentation: https://docs.qilbeedb.io
