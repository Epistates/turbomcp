# TurboMCP Production Deployment Guide

## 🚀 Overview

This guide covers basic production deployment of TurboMCP servers with security, monitoring, and scalability best practices.

## 🔧 Environment Variables

TurboMCP automatically loads configuration from environment variables, making it easy to deploy across different environments without code changes.

### Core Configuration

```bash
# Server Configuration
export MCP_SERVER_NAME="YourProductionServer"
export MCP_SERVER_VERSION="2.0.0"
export MCP_SERVER_DESCRIPTION="Production MCP Server"
export MCP_BIND_ADDRESS="0.0.0.0"
export MCP_PORT="8080"

# Request Limits
export MCP_MAX_REQUEST_SIZE="4194304"    # 4MB for production
export MCP_REQUEST_TIMEOUT="15"          # 15 seconds
export MCP_MAX_CONNECTIONS="200"         # Connection limit
```

### 🛡️ Security Configuration

#### TLS/SSL Settings
```bash
# TLS Certificate Configuration
export TLS_CERT_FILE="/etc/ssl/certs/server.pem"
export TLS_KEY_FILE="/etc/ssl/private/server.key"
export TLS_MIN_VERSION="1.3"              # Force TLS 1.3
export TLS_ENABLE_HTTP2="true"            # Enable HTTP/2
```

#### CORS Configuration
```bash
# Cross-Origin Resource Sharing
export CORS_ALLOWED_ORIGINS="https://app.example.com,https://admin.example.com"
export CORS_ALLOW_CREDENTIALS="true"
export CORS_MAX_AGE="600"                 # 10 minutes cache
```

#### Authentication Configuration
```bash
# JWT Authentication
export AUTH_JWT_SECRET="your-super-secure-256-bit-secret-key"
export AUTH_JWT_ALGORITHM="HS256"
export AUTH_JWT_EXPIRATION="3600"         # 1 hour

# API Key Authentication  
export AUTH_API_KEY_HEADER="X-API-Key"
export AUTH_ENABLED="true"
```

#### Rate Limiting Configuration
```bash
# Rate Limiting
export RATE_LIMIT_ENABLED="true"
export RATE_LIMIT_REQUESTS_PER_MINUTE="120"   # 2 requests per second
export RATE_LIMIT_BURST_CAPACITY="20"
export RATE_LIMIT_KEY_STRATEGY="ip"           # ip, user, or custom
```

#### Security Headers Configuration
```bash
# Content Security Policy
export CSP_POLICY="default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; media-src 'self'; frame-src 'none'"

# HTTP Strict Transport Security (2 years)
export HSTS_MAX_AGE="63072000"
export HSTS_INCLUDE_SUBDOMAINS="true"

# Additional Security Headers
export SECURITY_HEADERS_ENABLED="true"
export X_FRAME_OPTIONS="DENY"
export X_CONTENT_TYPE_OPTIONS="nosniff"
export REFERRER_POLICY="no-referrer"
export PERMISSIONS_POLICY="geolocation=(), microphone=(), camera=(), payment=(), usb=()"
```

### 📊 Observability & Monitoring

```bash
# Logging Configuration
export RUST_LOG="info,turbomcp=debug"
export LOG_FORMAT="json"                  # json or human-readable
export LOG_LEVEL="info"

# Metrics & Tracing
export METRICS_ENABLED="true"
export METRICS_PORT="9090"                # Prometheus metrics endpoint
export TRACING_ENABLED="true"
export JAEGER_ENDPOINT="http://jaeger:14268/api/traces"

# Health Check Configuration
export HEALTH_CHECK_ENABLED="true"
export HEALTH_CHECK_INTERVAL="30"         # seconds
```

### 💾 Data & Storage Configuration

```bash
# Database Configuration (if using persistent storage)
export DATABASE_URL="postgresql://user:password@db-host:5432/turbomcp"
export DATABASE_POOL_SIZE="10"
export DATABASE_TIMEOUT="30"

# Redis Configuration (for session storage, rate limiting)
export REDIS_URL="redis://redis-host:6379"
export REDIS_POOL_SIZE="10"
export REDIS_TIMEOUT="5"

# File Storage (for OAuth token encryption, if using file backend)
export OAUTH_ENCRYPTION_SECRET="your-256-bit-encryption-secret"
export OAUTH_TOKEN_STORAGE_PATH="/var/lib/turbomcp/tokens"
```

## 🐳 Docker Deployment

### Basic Docker Setup

```dockerfile
FROM rust:1.89-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev openssl-dev

WORKDIR /app
COPY . .

# Build release binary
RUN cargo build --release --features full

# Production runtime image
FROM alpine:latest

# Install runtime dependencies
RUN apk add --no-cache ca-certificates openssl

# Create non-root user
RUN adduser -D -s /bin/sh turbomcp

# Copy binary
COPY --from=builder /app/target/release/your-turbomcp-server /usr/local/bin/

# Set security-focused environment defaults
ENV TLS_MIN_VERSION=1.3
ENV RUST_LOG=info
ENV MCP_MAX_REQUEST_SIZE=4194304
ENV MCP_REQUEST_TIMEOUT=15
ENV MCP_MAX_CONNECTIONS=200

# Switch to non-root user
USER turbomcp

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://localhost:${MCP_PORT:-8080}/mcp/health || exit 1

EXPOSE 8080
CMD ["your-turbomcp-server"]
```

### Docker Compose with Security

```yaml
version: '3.8'

services:
  turbomcp-server:
    build: .
    ports:
      - "8080:8080"
      - "9090:9090"  # Metrics
    environment:
      # Server Configuration
      MCP_BIND_ADDRESS: "0.0.0.0"
      MCP_PORT: "8080"
      
      # Security Configuration
      TLS_CERT_FILE: "/etc/ssl/certs/server.pem"
      TLS_KEY_FILE: "/etc/ssl/private/server.key"
      CORS_ALLOWED_ORIGINS: "https://app.example.com"
      AUTH_JWT_SECRET: "${JWT_SECRET}"
      
      # Rate Limiting
      RATE_LIMIT_ENABLED: "true"
      RATE_LIMIT_REQUESTS_PER_MINUTE: "120"
      
      # Monitoring
      RUST_LOG: "info,turbomcp=debug"
      METRICS_ENABLED: "true"
      
    volumes:
      - ./ssl:/etc/ssl:ro
      - ./data:/var/lib/turbomcp
    secrets:
      - jwt_secret
    depends_on:
      - redis
      - postgres
    restart: unless-stopped
    
    # Security settings
    read_only: true
    tmpfs:
      - /tmp:size=100M,noexec,nosuid,nodev
    security_opt:
      - no-new-privileges:true
    user: "1000:1000"

  redis:
    image: redis:7-alpine
    command: redis-server --requirepass ${REDIS_PASSWORD}
    volumes:
      - redis_data:/data
    restart: unless-stopped

  postgres:
    image: postgres:15-alpine
    environment:
      POSTGRES_DB: turbomcp
      POSTGRES_USER: turbomcp_user
      POSTGRES_PASSWORD: "${DB_PASSWORD}"
    volumes:
      - postgres_data:/var/lib/postgresql/data
    restart: unless-stopped

volumes:
  redis_data:
  postgres_data:

secrets:
  jwt_secret:
    file: ./secrets/jwt_secret.txt
```

## ☸️ Kubernetes Deployment

### Kubernetes Manifests

```yaml
# ConfigMap for non-sensitive configuration
apiVersion: v1
kind: ConfigMap
metadata:
  name: turbomcp-config
data:
  MCP_BIND_ADDRESS: "0.0.0.0"
  MCP_PORT: "8080"
  MCP_MAX_REQUEST_SIZE: "4194304"
  MCP_REQUEST_TIMEOUT: "15"
  MCP_MAX_CONNECTIONS: "200"
  TLS_MIN_VERSION: "1.3"
  TLS_ENABLE_HTTP2: "true"
  RATE_LIMIT_ENABLED: "true"
  RATE_LIMIT_REQUESTS_PER_MINUTE: "120"
  RATE_LIMIT_BURST_CAPACITY: "20"
  SECURITY_HEADERS_ENABLED: "true"
  METRICS_ENABLED: "true"
  RUST_LOG: "info,turbomcp=debug"

---
# Secret for sensitive configuration
apiVersion: v1
kind: Secret
metadata:
  name: turbomcp-secrets
type: Opaque
stringData:
  AUTH_JWT_SECRET: "your-super-secure-jwt-secret"
  OAUTH_ENCRYPTION_SECRET: "your-256-bit-encryption-secret"
  DATABASE_URL: "postgresql://user:password@postgres:5432/turbomcp"
  REDIS_URL: "redis://:password@redis:6379"

---
# Deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: turbomcp-server
  labels:
    app: turbomcp-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: turbomcp-server
  template:
    metadata:
      labels:
        app: turbomcp-server
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
      containers:
      - name: turbomcp-server
        image: your-registry/turbomcp-server:latest
        ports:
        - containerPort: 8080
          name: http
        - containerPort: 9090
          name: metrics
        env:
        - name: CORS_ALLOWED_ORIGINS
          value: "https://app.example.com,https://admin.example.com"
        envFrom:
        - configMapRef:
            name: turbomcp-config
        - secretRef:
            name: turbomcp-secrets
        volumeMounts:
        - name: tls-certs
          mountPath: /etc/ssl/certs
          readOnly: true
        - name: tls-keys
          mountPath: /etc/ssl/private
          readOnly: true
        livenessProbe:
          httpGet:
            path: /mcp/health
            port: http
          initialDelaySeconds: 30
          periodSeconds: 30
        readinessProbe:
          httpGet:
            path: /mcp/health
            port: http
          initialDelaySeconds: 5
          periodSeconds: 10
        resources:
          requests:
            memory: "128Mi"
            cpu: "100m"
          limits:
            memory: "512Mi"
            cpu: "500m"
        securityContext:
          allowPrivilegeEscalation: false
          readOnlyRootFilesystem: true
          capabilities:
            drop:
            - ALL
      volumes:
      - name: tls-certs
        secret:
          secretName: tls-certificates
      - name: tls-keys
        secret:
          secretName: tls-private-keys

---
# Service
apiVersion: v1
kind: Service
metadata:
  name: turbomcp-service
spec:
  selector:
    app: turbomcp-server
  ports:
  - name: http
    port: 80
    targetPort: http
  - name: metrics
    port: 9090
    targetPort: metrics
  type: ClusterIP

---
# Ingress with TLS
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: turbomcp-ingress
  annotations:
    kubernetes.io/ingress.class: nginx
    nginx.ingress.kubernetes.io/ssl-redirect: "true"
    nginx.ingress.kubernetes.io/force-ssl-redirect: "true"
    cert-manager.io/cluster-issuer: "letsencrypt-prod"
spec:
  tls:
  - hosts:
    - api.example.com
    secretName: turbomcp-tls
  rules:
  - host: api.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: turbomcp-service
            port:
              number: 80
```

## 🔒 Security Best Practices

### 1. Certificate Management

```bash
# Generate self-signed certificate for testing
openssl req -x509 -newkey rsa:4096 -keyout server.key -out server.pem \
  -days 365 -nodes -subj "/CN=localhost"

# Production: Use Let's Encrypt or your CA
certbot certonly --standalone -d yourdomain.com
```

### 2. Secret Management

```bash
# Generate secure JWT secret
openssl rand -hex 32 > jwt_secret.txt

# Generate encryption secret for OAuth tokens
openssl rand -hex 32 > oauth_encryption_secret.txt

# Set appropriate file permissions
chmod 600 *.txt
```

### 3. Firewall Configuration

```bash
# Allow only necessary ports
ufw allow 80/tcp    # HTTP (redirect to HTTPS)
ufw allow 443/tcp   # HTTPS
ufw allow 22/tcp    # SSH (restrict by IP if possible)

# Deny all other traffic
ufw default deny incoming
ufw default allow outgoing
ufw enable
```

### 4. Monitoring & Alerting

```bash
# Prometheus configuration for metrics scraping
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
    - targets: ['turbomcp-server:9090']
    metrics_path: /mcp/metrics
    scrape_interval: 10s
```

## 📊 Performance Tuning

### System Configuration

```bash
# Increase file descriptor limits
echo "* soft nofile 65536" >> /etc/security/limits.conf
echo "* hard nofile 65536" >> /etc/security/limits.conf

# TCP tuning for high-performance networking
echo "net.core.rmem_max = 16777216" >> /etc/sysctl.conf
echo "net.core.wmem_max = 16777216" >> /etc/sysctl.conf
echo "net.ipv4.tcp_rmem = 4096 87380 16777216" >> /etc/sysctl.conf
echo "net.ipv4.tcp_wmem = 4096 65536 16777216" >> /etc/sysctl.conf
```

### Application Configuration

```bash
# Optimize for production workload
export MCP_MAX_CONNECTIONS="1000"
export MCP_WORKER_THREADS="8"
export MCP_REQUEST_TIMEOUT="10"
export MCP_KEEP_ALIVE="30"

# Enable SIMD acceleration
export CARGO_FEATURES="full,simd"
```

## 🚨 Security Checklist

### Pre-Deployment

- [ ] **TLS Certificates**: Valid certificates installed and configured
- [ ] **Environment Variables**: All secrets loaded from secure storage
- [ ] **CORS Origins**: Restricted to trusted domains only
- [ ] **Authentication**: JWT/API key authentication enabled
- [ ] **Rate Limiting**: Appropriate limits configured for your use case
- [ ] **Security Headers**: Full security headers suite enabled
- [ ] **Firewall Rules**: Only necessary ports exposed
- [ ] **User Permissions**: Running as non-root user

### Post-Deployment

- [ ] **Health Checks**: All endpoints responding correctly
- [ ] **TLS Testing**: SSL Labs test shows A+ rating
- [ ] **Security Scan**: No vulnerabilities in security scan
- [ ] **Performance Test**: Load testing completed successfully
- [ ] **Monitoring**: Metrics and logs flowing to monitoring systems
- [ ] **Backup**: Database and configuration backups configured
- [ ] **Documentation**: Runbook and incident response procedures

## 📚 Additional Resources

- [Security Features Documentation](./crates/turbomcp-transport/SECURITY_FEATURES.md)
- [API Documentation](https://docs.rs/turbomcp)
- [Model Context Protocol Specification](https://modelcontextprotocol.io/)

## 🤝 Support

For production deployment support:

1. **Issues**: Report problems via [GitHub Issues](https://github.com/Epistates/turbomcp/issues)
2. **Security**: Report security issues via [GitHub Security Advisories](https://github.com/Epistates/turbomcp/security/advisories)
3. **Documentation**: Contribute improvements to this deployment guide

---

**⚠️ Important**: Always test configuration changes in a staging environment before applying to production.