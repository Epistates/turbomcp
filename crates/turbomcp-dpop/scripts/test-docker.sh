#!/bin/bash

# Production-grade test infrastructure for TurboMCP DPoP
# Uses Docker for real Redis integration testing - no mocks!

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"
COMPOSE_FILE="$CRATE_DIR/docker-compose.test.yml"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

check_docker() {
    if ! command -v docker &> /dev/null; then
        log_error "Docker is not installed or not in PATH"
        exit 1
    fi

    if ! docker info &> /dev/null; then
        log_error "Docker daemon is not running"
        exit 1
    fi
}

check_docker_compose() {
    if ! command -v docker-compose &> /dev/null && ! docker compose version &> /dev/null 2>&1; then
        log_error "Docker Compose is not installed"
        exit 1
    fi
}

start_services() {
    log_info "Starting Redis test infrastructure..."
    
    cd "$CRATE_DIR"
    
    # Use docker compose or docker-compose based on availability
    if docker compose version &> /dev/null 2>&1; then
        COMPOSE_CMD="docker compose"
    else
        COMPOSE_CMD="docker-compose"
    fi
    
    $COMPOSE_CMD -f "$COMPOSE_FILE" down --volumes --remove-orphans
    $COMPOSE_CMD -f "$COMPOSE_FILE" up -d
    
    # Wait for Redis to be healthy
    log_info "Waiting for Redis to be ready..."
    local max_attempts=30
    local attempt=0
    
    while [ $attempt -lt $max_attempts ]; do
        if $COMPOSE_CMD -f "$COMPOSE_FILE" exec -T redis redis-cli -a turbomcp_test_password ping &> /dev/null; then
            log_success "Redis is ready!"
            break
        fi
        
        attempt=$((attempt + 1))
        if [ $attempt -eq $max_attempts ]; then
            log_error "Redis failed to start within timeout"
            $COMPOSE_CMD -f "$COMPOSE_FILE" logs redis
            exit 1
        fi
        
        log_info "Waiting for Redis... (attempt $attempt/$max_attempts)"
        sleep 2
    done
}

run_tests() {
    log_info "Running integration tests against real Redis..."
    
    cd "$CRATE_DIR"
    
    # Export Redis connection info for tests
    export REDIS_TEST_URL="redis://:turbomcp_test_password@localhost:16379"
    export REDIS_CLUSTER_TEST_URL="redis://:turbomcp_cluster_password@localhost:16380"
    
    # Run tests with Redis features enabled
    cargo test --features redis-storage,test-utils --test "*redis*" -- --test-threads=1
    
    # Run all tests with real infrastructure
    log_info "Running full test suite with Redis infrastructure..."
    cargo test --features redis-storage,test-utils,hsm-support -- --test-threads=1
}

cleanup() {
    log_info "Cleaning up test infrastructure..."
    
    cd "$CRATE_DIR"
    
    if docker compose version &> /dev/null 2>&1; then
        COMPOSE_CMD="docker compose"
    else
        COMPOSE_CMD="docker-compose"
    fi
    
    $COMPOSE_CMD -f "$COMPOSE_FILE" down --volumes --remove-orphans
    
    # Clean up any dangling volumes
    docker volume prune -f --filter label=com.docker.compose.project=turbomcp-dpop
}

show_logs() {
    log_info "Showing service logs..."
    
    cd "$CRATE_DIR"
    
    if docker compose version &> /dev/null 2>&1; then
        COMPOSE_CMD="docker compose"
    else
        COMPOSE_CMD="docker-compose"
    fi
    
    $COMPOSE_CMD -f "$COMPOSE_FILE" logs -f
}

main() {
    case "${1:-}" in
        "start")
            check_docker
            check_docker_compose
            start_services
            ;;
        "test")
            check_docker
            check_docker_compose
            start_services
            run_tests
            cleanup
            ;;
        "cleanup"|"clean")
            check_docker
            check_docker_compose
            cleanup
            ;;
        "logs")
            show_logs
            ;;
        "status")
            check_docker
            check_docker_compose
            cd "$CRATE_DIR"
            if docker compose version &> /dev/null 2>&1; then
                docker compose -f "$COMPOSE_FILE" ps
            else
                docker-compose -f "$COMPOSE_FILE" ps
            fi
            ;;
        *)
            echo "Usage: $0 {start|test|cleanup|logs|status}"
            echo ""
            echo "Commands:"
            echo "  start   - Start Redis test infrastructure"
            echo "  test    - Run integration tests against real Redis"
            echo "  cleanup - Stop and remove all test infrastructure"
            echo "  logs    - Show service logs"
            echo "  status  - Show service status"
            echo ""
            echo "This script provides production-grade testing against real Redis infrastructure."
            echo "No mocks, no fakes - just real services in Docker containers."
            exit 1
            ;;
    esac
}

main "$@"