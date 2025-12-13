# TurboMCP Documentation

Complete documentation site for TurboMCP built with MkDocs Material.

## Structure

```
docs/
├── index.md                    # Homepage
├── getting-started/            # Getting started guides
│   ├── overview.md             # Overview and key concepts
│   ├── installation.md         # Installation instructions
│   ├── quick-start.md          # 5-minute tutorial
│   └── first-server.md         # Complete first server example
├── guide/                      # Complete guides
│   ├── architecture.md         # TurboMCP architecture
│   ├── handlers.md             # Defining handlers
│   ├── context-injection.md    # Context and DI
│   ├── transports.md           # Transport configuration
│   ├── authentication.md       # Authentication setup
│   ├── observability.md        # Logging and monitoring
│   └── advanced-patterns.md    # Advanced patterns
├── api/                        # API reference
│   ├── protocol.md             # Protocol layer
│   ├── server.md               # Server framework
│   ├── client.md               # Client implementation
│   ├── macros.md               # Macro reference
│   └── utilities.md            # Utility types
├── examples/                   # Examples and patterns
│   ├── basic.md                # Basic examples
│   ├── patterns.md             # Real-world patterns
│   └── advanced.md             # Advanced examples
├── deployment/                 # Deployment guides
│   ├── docker.md               # Docker deployment
│   ├── production.md           # Production setup
│   └── monitoring.md           # Monitoring and metrics
├── architecture/               # Architecture deep dives
│   ├── system-design.md        # System design
│   ├── context-lifecycle.md    # Context lifecycle
│   ├── dependency-injection.md # DI implementation
│   └── protocol-compliance.md  # Protocol compliance
└── contributing/               # Contributing guides
    ├── code-of-conduct.md      # Code of conduct
    ├── development.md          # Development setup
    └── documentation.md        # Documentation guidelines
```

## Building

### Prerequisites

- Python 3.8+
- mkdocs
- mkdocs-material

### Install Dependencies

```bash
pip install mkdocs mkdocs-material
```

### Run Locally

```bash
mkdocs serve
```

Visit http://localhost:8000 in your browser.

### Build Static Site

```bash
mkdocs build
```

This creates a `site/` directory with the static HTML.

## Content Status

### Complete ✅
- [x] Homepage (index.md)
- [x] Overview (getting-started/overview.md)
- [x] Installation (getting-started/installation.md)
- [x] Quick Start (getting-started/quick-start.md)
- [x] First Server (getting-started/first-server.md)
- [x] Architecture (guide/architecture.md)
- [x] Handlers (guide/handlers.md)

### In Progress 🔄
- [ ] Context & DI guide
- [ ] Transport guide
- [ ] Authentication guide
- [ ] API references
- [ ] Deployment guides
- [ ] Architecture deep dives

### Planned 📋
- [ ] More examples
- [ ] Contributing guidelines
- [ ] Advanced patterns

## Contributing to Docs

1. Edit markdown files in the `docs/` directory
2. Run `mkdocs serve` to preview changes
3. Commit and push changes to GitHub

## Configuration

See `mkdocs.yml` in the root directory for:
- Site metadata
- Theme configuration
- Navigation structure
- Extensions and plugins

## Deployment

The documentation can be deployed to:
- GitHub Pages
- Netlify
- Vercel
- Any static host

See deployment guides for details.
