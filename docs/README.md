# QilbeeDB Documentation

This directory contains the source files for the QilbeeDB documentation website, built with MkDocs Material.

## Viewing the Documentation

### Local Development Server

To view the documentation locally with live reload:

```bash
# Activate virtual environment
source docs-venv/bin/activate

# Serve documentation
mkdocs serve
```

Then open your browser to: http://127.0.0.1:8000

### Build Static Site

To build the documentation as static HTML:

```bash
# Activate virtual environment
source docs-venv/bin/activate

# Build site
mkdocs build
```

The built site will be in the `site/` directory.

## Documentation Structure

```
docs/
├── index.md                    # Home page
├── getting-started/
│   ├── installation.md        # Installation guide
│   ├── quickstart.md          # Quick start tutorial
│   └── configuration.md       # Configuration options
├── client-libraries/
│   └── python.md              # Python SDK documentation
├── agent-memory/
│   ├── overview.md            # Agent memory overview
│   ├── episodes.md            # Episode types and usage
│   ├── memory-types.md        # Memory type architecture
│   ├── consolidation.md       # Memory consolidation
│   ├── forgetting.md          # Active forgetting
│   └── statistics.md          # Memory statistics
└── ... (additional sections)
```

## Contributing to Documentation

### Prerequisites

- Python 3.8+
- Virtual environment with MkDocs installed

### Setup

```bash
# Create virtual environment (if not exists)
python3 -m venv docs-venv

# Activate virtual environment
source docs-venv/bin/activate

# Install dependencies
pip install mkdocs mkdocs-material
```

### Adding New Pages

1. Create a new Markdown file in the appropriate directory
2. Add the page to `mkdocs.yml` navigation
3. Test locally with `mkdocs serve`
4. Build with `mkdocs build` to verify

### Markdown Guidelines

- Use GitHub-flavored Markdown
- Include code examples with syntax highlighting
- Add navigation links at the bottom of pages
- Use clear section headings (##, ###)
- Include practical examples

## Configuration

The documentation site is configured in `mkdocs.yml`:

- **Theme**: Material theme with dark/light mode
- **Features**: Navigation tabs, search, code copying
- **Extensions**: Syntax highlighting, tabbed content, admonitions

## Deployment

To deploy the documentation:

```bash
# Build production site
mkdocs build

# Deploy to GitHub Pages (if configured)
mkdocs gh-deploy
```

## Documentation Status

### Complete Pages
- ✅ Home (index.md)
- ✅ Installation guide
- ✅ Quick start tutorial
- ✅ Configuration reference
- ✅ Python SDK documentation
- ✅ Agent Memory overview
- ✅ Episodes documentation
- ✅ Memory Types documentation

### In Progress
- 🔄 Consolidation
- 🔄 Forgetting
- 🔄 Statistics

### Planned
- Graph Operations (nodes, relationships, properties, indexes, transactions)
- Cypher Query Language (MATCH, WHERE, RETURN, CREATE, etc.)
- Use Cases (AI agents, social networks, knowledge graphs, etc.)
- Architecture (storage, query engine, memory engine, bi-temporal)
- API Reference (HTTP, Bolt, Graph API, Memory API)
- Operations (deployment, Docker, monitoring, backup)
- Contributing

## Need Help?

- Report documentation issues on GitHub
- Suggest improvements via pull requests
- Ask questions in GitHub discussions
