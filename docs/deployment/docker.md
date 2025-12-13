# Docker Deployment

Complete guide to deploying TurboMCP servers with Docker, including production-optimized Dockerfiles, docker-compose configurations, and best practices.

## Quick Start

### Basic Dockerfile

Minimal production-ready Dockerfile for TurboMCP servers:

```dockerfile
# Multi-stage build for minimal image size
FROM rust:1.89-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy main.rs to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy actual source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 appuser

# Copy binary from builder
COPY --from=builder /app/target/release/your-mcp-server /usr/local/bin/server

# Switch to non-root user
USER appuser

EXPOSE 8080

CMD ["server"]
```

**Build and run:**

```bash
# Build image
docker build -t mcp-server:latest .

# Run container
docker run -p 8080:8080 mcp-server:latest
```

## Optimized Production Dockerfile

Enhanced Dockerfile with caching, security, and size optimizations:

```dockerfile
# syntax=docker/dockerfile:1.4

# Stage 1: Cache dependencies
FROM rust:1.89-slim as dependencies

# Install build tools
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    musl-tools \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/*/Cargo.toml ./crates/

# Create dummy source files to build dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    find crates -name "Cargo.toml" -exec dirname {} \; | \
    while read dir; do mkdir -p "$dir/src" && echo "" > "$dir/src/lib.rs"; done

# Build dependencies (cached layer)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release

# Stage 2: Build application
FROM dependencies as builder

# Remove dummy files
RUN rm -rf src crates/*/src

# Copy actual source code
COPY src ./src
COPY crates ./crates

# Build with all optimizations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/your-mcp-server /server

# Stage 3: Runtime (distroless for minimal attack surface)
FROM gcr.io/distroless/cc-debian12

# Copy CA certificates
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy binary
COPY --from=builder /server /usr/local/bin/server

# Run as non-root
USER nonroot:nonroot

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/server"]
```

**Key Optimizations:**
- BuildKit cache mounts for faster rebuilds
- Separate dependency caching layer
- Distroless runtime image (< 20MB)
- Non-root user
- Minimal attack surface

## Docker Compose Configurations

### Development Setup

Complete development environment with hot reload:

```yaml
version: '3.8'

services:
  mcp-server:
    build:
      context: .
      target: builder
    command: cargo watch -x run
    volumes:
      - .:/app
      - cargo-cache:/usr/local/cargo/registry
      - target-cache:/app/target
    ports:
      - "8080:8080"
    environment:
      - RUST_LOG=debug
      - RUST_BACKTRACE=1
    networks:
      - mcp-network

  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: mcp_dev
      POSTGRES_USER: mcp
      POSTGRES_PASSWORD: dev_password
    ports:
      - "5432:5432"
    volumes:
      - postgres-data:/var/lib/postgresql/data
    networks:
      - mcp-network
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U mcp"]
      interval: 10s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis-data:/data
    networks:
      - mcp-network
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 3s
      retries: 5

volumes:
  cargo-cache:
  target-cache:
  postgres-data:
  redis-data:

networks:
  mcp-network:
    driver: bridge
```

**Usage:**

```bash
# Start all services
docker-compose up -d

# View logs
docker-compose logs -f mcp-server

# Rebuild after dependency changes
docker-compose build mcp-server

# Stop all services
docker-compose down
```

### Production Setup

Production-ready docker-compose with health checks, restart policies, and resource limits:

```yaml
version: '3.8'

services:
  mcp-server:
    image: your-registry/mcp-server:${VERSION:-latest}
    restart: unless-stopped
    ports:
      - "8080:8080"
    environment:
      - RUST_LOG=info
      - DATABASE_URL=postgresql://mcp:${DB_PASSWORD}@postgres:5432/mcp_prod
      - REDIS_URL=redis://redis:6379
      - SERVER_PORT=8080
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
    networks:
      - mcp-network
    healthcheck:
      test: ["CMD", "wget", "--spider", "-q", "http://localhost:8080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
        reservations:
          cpus: '1'
          memory: 1G
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"

  postgres:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: mcp_prod
      POSTGRES_USER: mcp
      POSTGRES_PASSWORD: ${DB_PASSWORD}
      POSTGRES_INITDB_ARGS: "-E UTF8 --locale=C"
    volumes:
      - postgres-data:/var/lib/postgresql/data
      - ./init-db:/docker-entrypoint-initdb.d:ro
    networks:
      - mcp-network
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U mcp"]
      interval: 10s
      timeout: 5s
      retries: 5
    deploy:
      resources:
        limits:
          cpus: '1'
          memory: 1G

  redis:
    image: redis:7-alpine
    restart: unless-stopped
    command: redis-server --appendonly yes --requirepass ${REDIS_PASSWORD}
    volumes:
      - redis-data:/data
    networks:
      - mcp-network
    healthcheck:
      test: ["CMD", "redis-cli", "--raw", "incr", "ping"]
      interval: 10s
      timeout: 3s
      retries: 5
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 512M

  nginx:
    image: nginx:alpine
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
      - ./ssl:/etc/nginx/ssl:ro
    depends_on:
      - mcp-server
    networks:
      - mcp-network
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 256M

volumes:
  postgres-data:
    driver: local
  redis-data:
    driver: local

networks:
  mcp-network:
    driver: bridge
```

**Production deployment:**

```bash
# Set environment variables
export VERSION=1.0.0
export DB_PASSWORD=$(openssl rand -base64 32)
export REDIS_PASSWORD=$(openssl rand -base64 32)

# Pull and start
docker-compose pull
docker-compose up -d

# Monitor
docker-compose ps
docker-compose logs -f
```

## Multi-Architecture Builds

Build images for multiple platforms (AMD64, ARM64):

```dockerfile
# Use buildx for multi-arch
FROM --platform=$BUILDPLATFORM rust:1.89-slim as builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install cross-compilation tools
RUN case "$TARGETPLATFORM" in \
    "linux/amd64") echo "x86_64-unknown-linux-gnu" > /rust-target.txt ;; \
    "linux/arm64") echo "aarch64-unknown-linux-gnu" > /rust-target.txt ;; \
    *) echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
    esac && \
    rustup target add $(cat /rust-target.txt)

COPY . .

RUN cargo build --release --target $(cat /rust-target.txt) && \
    cp target/$(cat /rust-target.txt)/release/your-mcp-server /server

FROM --platform=$TARGETPLATFORM debian:bookworm-slim

COPY --from=builder /server /usr/local/bin/server

USER 1000:1000

EXPOSE 8080

CMD ["server"]
```

**Build for multiple architectures:**

```bash
# Create buildx builder
docker buildx create --name multiarch --use

# Build and push
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t your-registry/mcp-server:latest \
  --push \
  .
```

## Environment-Specific Configurations

### Development (.env.development)

```bash
# Development environment
RUST_LOG=debug
RUST_BACKTRACE=full

# Server
SERVER_HOST=0.0.0.0
SERVER_PORT=8080
SERVER_WORKERS=4

# Database
DATABASE_URL=postgresql://mcp:dev_password@postgres:5432/mcp_dev
DATABASE_MAX_CONNECTIONS=10

# Redis
REDIS_URL=redis://redis:6379

# Features
ENABLE_METRICS=true
ENABLE_TRACING=true
CORS_ALLOW_ORIGINS=*
```

### Production (.env.production)

```bash
# Production environment
RUST_LOG=info

# Server
SERVER_HOST=0.0.0.0
SERVER_PORT=8080
SERVER_WORKERS=0  # Auto-detect CPU count

# Database
DATABASE_URL=postgresql://mcp:${DB_PASSWORD}@postgres:5432/mcp_prod
DATABASE_MAX_CONNECTIONS=50
DATABASE_MIN_CONNECTIONS=10

# Redis
REDIS_URL=redis://:${REDIS_PASSWORD}@redis:6379

# Security
CORS_ALLOW_ORIGINS=https://yourdomain.com
RATE_LIMIT_REQUESTS=100
RATE_LIMIT_WINDOW=60

# Features
ENABLE_METRICS=true
ENABLE_TRACING=true
ENABLE_PROFILING=false
```

## Image Optimization Techniques

### Minimal Alpine-Based Image

Ultra-small image using Alpine Linux:

```dockerfile
FROM rust:1.89-alpine as builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build static binary
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN cargo build --release --target x86_64-unknown-linux-musl

# Runtime
FROM alpine:latest

RUN apk add --no-cache ca-certificates

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/your-mcp-server /server

USER 1000:1000

CMD ["/server"]
```

**Result:** Image size < 15MB

### Distroless for Maximum Security

Google's distroless images for minimal attack surface:

```dockerfile
FROM rust:1.89-slim as builder
# ... build steps ...

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /app/target/release/your-mcp-server /server
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

USER nonroot:nonroot

ENTRYPOINT ["/server"]
```

**Benefits:**
- No shell, package manager, or utilities
- Minimal CVE surface
- ~20MB total size

## Health Checks

### Application Health Endpoint

Implement comprehensive health checks:

```rust
use axum::{Router, Json};
use serde_json::json;

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn readiness_check(db: Pool<Postgres>, redis: RedisClient) -> Json<serde_json::Value> {
    let db_healthy = db.acquire().await.is_ok();
    let redis_healthy = redis.ping().await.is_ok();

    let status = if db_healthy && redis_healthy { "ready" } else { "not_ready" };

    Json(json!({
        "status": status,
        "checks": {
            "database": db_healthy,
            "redis": redis_healthy,
        }
    }))
}

fn app() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        // ... other routes
}
```

### Docker Health Check

```dockerfile
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1
```

Or using wget (smaller):

```dockerfile
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD wget --spider -q http://localhost:8080/health || exit 1
```

## Security Best Practices

### Secure Dockerfile

```dockerfile
FROM rust:1.89-slim as builder

# Scan base image
# docker scan rust:1.89-slim

# Use specific versions
RUN apt-get update && apt-get install -y \
    pkg-config=1.8.1-1 \
    libssl-dev=3.0.11-1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only necessary files
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build with security flags
ENV RUSTFLAGS="-D warnings -C target-cpu=native"
RUN cargo build --release

# Verify binary
RUN ldd target/release/your-mcp-server

FROM gcr.io/distroless/cc-debian12

# Don't run as root
USER nonroot:nonroot

# Read-only filesystem
COPY --from=builder --chown=nonroot:nonroot /app/target/release/your-mcp-server /server

# Drop all capabilities
# Set in docker-compose or k8s manifests

ENTRYPOINT ["/server"]
```

### Security Scanning

```bash
# Scan image for vulnerabilities
docker scan your-registry/mcp-server:latest

# Use Trivy for comprehensive scanning
trivy image your-registry/mcp-server:latest

# Scan during CI/CD
docker build -t mcp-server:latest .
trivy image --exit-code 1 --severity HIGH,CRITICAL mcp-server:latest
```

## CI/CD Integration

### GitHub Actions

```yaml
name: Build and Push Docker Image

on:
  push:
    branches: [main]
    tags: ['v*']

jobs:
  build:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to Container Registry
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/${{ github.repository }}
          tags: |
            type=ref,event=branch
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}
            type=sha

      - name: Build and push
        uses: docker/build-push-action@v5
        with:
          context: .
          platforms: linux/amd64,linux/arm64
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=gha
          cache-to: type=gha,mode=max

      - name: Scan image
        uses: aquasecurity/trivy-action@master
        with:
          image-ref: ghcr.io/${{ github.repository }}:${{ steps.meta.outputs.version }}
          format: 'sarif'
          output: 'trivy-results.sarif'

      - name: Upload scan results
        uses: github/codeql-action/upload-sarif@v2
        with:
          sarif_file: 'trivy-results.sarif'
```

## Troubleshooting

### Common Issues

**Issue:** Build fails with "cannot find -lssl"

```dockerfile
# Solution: Install libssl-dev
RUN apt-get update && apt-get install -y libssl-dev
```

**Issue:** Binary not found in runtime stage

```dockerfile
# Solution: Verify binary path
RUN ls -la /app/target/release/
COPY --from=builder /app/target/release/your-actual-binary /server
```

**Issue:** Permission denied

```dockerfile
# Solution: Set correct permissions
COPY --from=builder --chown=nonroot:nonroot /app/target/release/server /server
```

**Issue:** Slow builds

```bash
# Solution: Use BuildKit cache
export DOCKER_BUILDKIT=1
docker build --cache-from your-registry/mcp-server:latest .
```

### Debugging Containers

```bash
# View logs
docker logs mcp-server

# Execute shell (if available)
docker exec -it mcp-server sh

# Inspect container
docker inspect mcp-server

# Check resource usage
docker stats mcp-server

# View health check status
docker inspect --format='{{json .State.Health}}' mcp-server | jq
```

## Performance Tuning

### Resource Limits

```yaml
services:
  mcp-server:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
          pids: 100
        reservations:
          cpus: '1'
          memory: 1G
    ulimits:
      nofile:
        soft: 65536
        hard: 65536
      nproc: 65535
```

### Build Performance

```dockerfile
# Use cargo-chef for better caching
FROM lukemathwalker/cargo-chef:latest-rust-1.89 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies (cached layer)
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release
```

## See Also

- [Production Deployment](./production.md) - Production configuration and best practices
- [Monitoring](./monitoring.md) - Observability and metrics
- [Docker Documentation](https://docs.docker.com/) - Official Docker docs
- [Docker Best Practices](https://docs.docker.com/develop/dev-best-practices/) - Docker guidelines
