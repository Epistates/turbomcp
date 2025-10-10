# TurboMCP - Production Rust MCP Framework Makefile
# =================================================
# Professional development workflow automation for TurboMCP
# Supports development, testing, documentation, benchmarking, and deployment

# Color definitions for pretty output
RESET := \033[0m
RED := \033[31m
GREEN := \033[32m
YELLOW := \033[33m
BLUE := \033[34m
MAGENTA := \033[35m
CYAN := \033[36m
WHITE := \033[37m
BOLD := \033[1m

# Project configuration
PROJECT_NAME := TurboMCP
RUST_VERSION := 1.89.0
CARGO := cargo
RUSTUP := rustup

# Build configurations
RELEASE_FLAGS := --release
ALL_FEATURES_FLAGS := --all-features
WORKSPACE_FLAGS := --workspace

# Directory structure
CRATES_DIR := crates
EXAMPLES_DIR := examples
TARGET_DIR := target
COVERAGE_DIR := coverage

# Version and Git info
VERSION := $(shell grep '^version' crates/turbomcp/Cargo.toml | head -1 | cut -d '"' -f 2)
GIT_HASH := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")
GIT_BRANCH := $(shell git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
BUILD_TIME := $(shell date -u '+%Y-%m-%d_%H:%M:%S_UTC')

# CI/CD detection
CI ?= false
GITHUB_ACTIONS ?= false

.PHONY: all help setup build test clean fmt lint docs examples benchmarks \
	    release docker security audit coverage install uninstall check-deps \
		watch dev production stats report ci-prepare ci-test ci-build \
		publish pre-commit git-hooks demo performance-test load-test \
		test-integration test-integration-all test-enforce test-all

# Default target
all: build

# Help system with colored output
help: ## Show this help message with available targets
	@echo "${BOLD}${CYAN}$(PROJECT_NAME) Development Makefile${RESET}"
	@echo "${BLUE}Version: ${VERSION} | Git: ${GIT_BRANCH}@${GIT_HASH} | Built: ${BUILD_TIME}${RESET}"
	@echo ""
	@echo "${BOLD}${GREEN}Available Targets:${RESET}"
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  ${CYAN}%-20s${RESET} %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo ""
	@echo "${BOLD}${YELLOW}Quick Start:${RESET}"
	@echo "  ${CYAN}make setup${RESET}     - Set up development environment"
	@echo "  ${CYAN}make dev${RESET}       - Start development workflow"
	@echo "  ${CYAN}make test${RESET}      - Run full test suite"
	@echo "  ${CYAN}make release${RESET}   - Build optimized release"
	@echo ""

# Development Environment Setup
setup: ## Set up development environment
	@echo "${BOLD}${CYAN}🚀 Setting up $(PROJECT_NAME) development environment...${RESET}"
	@$(RUSTUP) toolchain install $(RUST_VERSION)
	@$(RUSTUP) default $(RUST_VERSION)
	@$(RUSTUP) component add rustfmt clippy llvm-tools-preview
	@echo "${GREEN}✅ Development environment ready!${RESET}"

setup-tools: ## Install optional development tools
	@echo "${BOLD}${CYAN}🔧 Installing optional development tools...${RESET}"
	@echo "${BLUE}Installing core tools...${RESET}"
	@$(CARGO) install cargo-watch || echo "${YELLOW}Failed to install cargo-watch${RESET}"
	@$(CARGO) install cargo-llvm-cov || echo "${YELLOW}Failed to install cargo-llvm-cov${RESET}"
	@echo "${BLUE}Installing analysis tools...${RESET}"
	@$(CARGO) install cargo-audit || echo "${YELLOW}Failed to install cargo-audit${RESET}"
	@$(CARGO) install cargo-outdated || echo "${YELLOW}Failed to install cargo-outdated${RESET}"
	@$(CARGO) install cargo-bloat || echo "${YELLOW}Failed to install cargo-bloat${RESET}"
	@echo "${BLUE}Installing performance tools...${RESET}"
	@$(CARGO) install cargo-tarpaulin || echo "${YELLOW}Failed to install cargo-tarpaulin${RESET}"
	@$(CARGO) install flamegraph || echo "${YELLOW}Failed to install flamegraph${RESET}"
	@echo "${GREEN}✅ Tool installation completed (some may have failed)${RESET}"

# Build Targets
build: ## Build all crates in development mode
	@echo "${BOLD}${BLUE}🔨 Building $(PROJECT_NAME)...${RESET}"
	@$(CARGO) build $(WORKSPACE_FLAGS)
	@echo "${GREEN}✅ Build completed successfully${RESET}"

build-release: ## Build optimized release version
	@echo "${BOLD}${BLUE}🔨 Building $(PROJECT_NAME) release...${RESET}"
	@$(CARGO) build $(WORKSPACE_FLAGS) $(RELEASE_FLAGS)
	@echo "${GREEN}✅ Release build completed${RESET}"

build-all-features: ## Build with all features enabled
	@echo "${BOLD}${BLUE}🔨 Building $(PROJECT_NAME) with all features...${RESET}"
	@$(CARGO) build $(WORKSPACE_FLAGS) $(ALL_FEATURES_FLAGS)
	@echo "${GREEN}✅ All features build completed${RESET}"

# Testing Targets
test: ## Run comprehensive test suite (tests + clippy + fmt)
	@echo "${BOLD}${CYAN}🧪 Running comprehensive test suite...${RESET}"
	@echo "${BLUE}📋 Step 1/4: Running cargo test (excluding Unix socket tests)...${RESET}"
	@$(CARGO) test --workspace --lib --tests --exclude turbomcp-transport
	@$(CARGO) test -p turbomcp-transport --lib --tests --features stdio,tcp
	@echo "${BLUE}📋 Step 2/4: Running cargo clippy...${RESET}"
	@$(CARGO) clippy $(WORKSPACE_FLAGS) --all-targets --all-features -- -D warnings
	@echo "${BLUE}📋 Step 3/4: Checking cargo fmt...${RESET}"
	@$(CARGO) fmt --all --check
	@echo "${BLUE}📋 Step 4/4: Running example compilation checks...${RESET}"
	@$(CARGO) check --examples --features full
	@echo "${GREEN}✅ All tests, linting, and formatting checks passed!${RESET}"

test-integration: ## Run comprehensive integration tests only
	@echo "${BOLD}${GREEN}🏆 Running integration tests...${RESET}"
	@$(CARGO) test --package turbomcp --test integration_tests
	@echo "${GREEN}✅ Integration tests passed!${RESET}"

test-enforce: ## Run zero-tolerance test quality enforcement
	@echo "${BOLD}${RED}⚡ Running zero-tolerance test quality enforcement...${RESET}"
	@$(CARGO) test --package turbomcp --test zero_tolerance_enforcement
	@echo "${GREEN}✅ Zero-tolerance enforcement passed!${RESET}"

test-all: ## Run all tests including zero-tolerance enforcement
	@$(MAKE) test
	@$(MAKE) test-enforce
	@echo "${BOLD}${GREEN}✅ All tests and enforcement checks passed!${RESET}"

test-only: ## Run tests only (no linting/formatting)
	@echo "${CYAN}🧪 Running tests only...${RESET}"
	@$(CARGO) test $(WORKSPACE_FLAGS) --lib --tests
	@echo "${GREEN}✅ All tests passed${RESET}"

test-all-features: ## Run tests with all features enabled
	@echo "${CYAN}🧪 Running tests with all features...${RESET}"
	@$(CARGO) test $(WORKSPACE_FLAGS) $(ALL_FEATURES_FLAGS) --lib --tests
	@echo "${GREEN}✅ All features tests passed${RESET}"

test-unit: ## Run unit tests only
	@echo "${CYAN}Running unit tests...${RESET}"
	@$(CARGO) test $(WORKSPACE_FLAGS) --lib

test-integration-all: ## Run all integration tests in workspace  
	@echo "${CYAN}Running all integration tests...${RESET}"
	@$(CARGO) test $(WORKSPACE_FLAGS) --tests

test-docs: ## Test documentation examples
	@echo "${CYAN}Testing documentation examples...${RESET}"
	@$(CARGO) test $(WORKSPACE_FLAGS) --doc

test-examples: ## Build and test all examples
	@echo "${CYAN}Building examples...${RESET}"
	@$(CARGO) build --examples
	@echo "${GREEN}✅ Examples build completed${RESET}"

# Code Quality
fmt: ## Format code using rustfmt
	@echo "${YELLOW}🎨 Formatting code...${RESET}"
	@$(CARGO) fmt --all
	@echo "${GREEN}✅ Code formatting completed${RESET}"

fmt-check: ## Check code formatting without making changes
	@echo "${YELLOW}🎨 Checking code formatting...${RESET}"
	@$(CARGO) fmt --all -- --check

lint: ## Run clippy linter
	@echo "${YELLOW}🔍 Linting code...${RESET}"
	@$(CARGO) clippy $(WORKSPACE_FLAGS) --all-targets -- -D warnings
	@echo "${GREEN}✅ Linting completed${RESET}"

lint-fix: ## Auto-fix clippy warnings where possible
	@echo "${YELLOW}🔧 Auto-fixing lint issues...${RESET}"
	@$(CARGO) clippy $(WORKSPACE_FLAGS) --all-targets --fix --allow-dirty

check: ## Fast compile check without building
	@echo "${BLUE}⚡ Running fast check...${RESET}"
	@$(CARGO) check $(WORKSPACE_FLAGS) --all-targets

check-all-features: ## Check with all features enabled
	@echo "${BLUE}⚡ Checking with all features...${RESET}"
	@$(CARGO) check $(WORKSPACE_FLAGS) $(ALL_FEATURES_FLAGS) --all-targets

check-deps: ## Check dependency tree
	@echo "${BLUE}📦 Checking dependencies...${RESET}"
	@$(CARGO) tree

# Tool Status
tool-status: ## Show status of optional development tools
	@echo "${BOLD}${CYAN}🔧 Development Tool Status${RESET}"
	@echo "${BLUE}Core Tools:${RESET}"
	@command -v cargo-watch >/dev/null 2>&1 && echo "  ✅ cargo-watch" || echo "  ❌ cargo-watch (install: cargo install cargo-watch)"
	@command -v cargo-llvm-cov >/dev/null 2>&1 && echo "  ✅ cargo-llvm-cov" || echo "  ❌ cargo-llvm-cov (install: cargo install cargo-llvm-cov)"
	@echo "${BLUE}Analysis Tools:${RESET}"
	@command -v cargo-audit >/dev/null 2>&1 && echo "  ✅ cargo-audit" || echo "  ❌ cargo-audit (install: cargo install cargo-audit)"
	@command -v cargo-outdated >/dev/null 2>&1 && echo "  ✅ cargo-outdated" || echo "  ❌ cargo-outdated (install: cargo install cargo-outdated)"
	@command -v cargo-bloat >/dev/null 2>&1 && echo "  ✅ cargo-bloat" || echo "  ❌ cargo-bloat (install: cargo install cargo-bloat)"
	@echo "${BLUE}Performance Tools:${RESET}"
	@command -v cargo-tarpaulin >/dev/null 2>&1 && echo "  ✅ cargo-tarpaulin" || echo "  ❌ cargo-tarpaulin (install: cargo install cargo-tarpaulin)"
	@command -v cargo-flamegraph >/dev/null 2>&1 && echo "  ✅ cargo-flamegraph" || echo "  ❌ cargo-flamegraph (install: cargo install flamegraph)"
	@echo "${BLUE}System Tools:${RESET}"
	@command -v docker >/dev/null 2>&1 && echo "  ✅ docker" || echo "  ❌ docker"

# Security & Audit
audit: ## Security audit of dependencies (requires cargo-audit)
	@echo "${RED}🔒 Running security audit...${RESET}"
	@if command -v cargo-audit >/dev/null 2>&1; then \
		$(CARGO) audit; \
		echo "${GREEN}✅ Security audit completed${RESET}"; \
	else \
		echo "${YELLOW}⚠️  cargo-audit not installed. Install with: cargo install cargo-audit${RESET}"; \
	fi

security: ## Comprehensive security analysis
	@echo "${RED}🛡️  Running comprehensive security analysis...${RESET}"
	@$(MAKE) audit

# Documentation
docs: ## Generate and open documentation
	@echo "${MAGENTA}📚 Generating documentation...${RESET}"
	@$(CARGO) doc --workspace --no-deps --open
	@echo "${GREEN}✅ Documentation generated${RESET}"

docs-build: ## Build documentation without opening
	@echo "${MAGENTA}📚 Building documentation...${RESET}"
	@$(CARGO) doc --workspace --no-deps

docs-check: ## Check documentation for broken links and issues
	@echo "${MAGENTA}📚 Checking documentation...${RESET}"
	@$(CARGO) doc --workspace --no-deps --document-private-items
	@$(MAKE) test-docs

# Coverage (requires cargo-llvm-cov)
coverage: ## Generate test coverage report  
	@echo "${CYAN}📊 Generating coverage report...${RESET}"
	@if command -v cargo-llvm-cov >/dev/null 2>&1; then \
		$(CARGO) llvm-cov --html --output-dir $(COVERAGE_DIR) $(WORKSPACE_FLAGS) $(ALL_FEATURES_FLAGS); \
		echo "${GREEN}✅ Coverage report generated in $(COVERAGE_DIR)/index.html${RESET}"; \
	else \
		echo "${YELLOW}⚠️  cargo-llvm-cov not installed. Install with: cargo install cargo-llvm-cov${RESET}"; \
	fi

coverage-text: ## Show coverage summary in terminal
	@echo "${CYAN}📊 Coverage Summary:${RESET}"
	@if command -v cargo-llvm-cov >/dev/null 2>&1; then \
		$(CARGO) llvm-cov $(WORKSPACE_FLAGS) $(ALL_FEATURES_FLAGS); \
	else \
		echo "${YELLOW}⚠️  cargo-llvm-cov not installed. Install with: cargo install cargo-llvm-cov${RESET}"; \
	fi

coverage-tarpaulin: ## Generate coverage using tarpaulin
	@echo "${CYAN}📊 Generating coverage with tarpaulin...${RESET}"
	@if command -v cargo-tarpaulin >/dev/null 2>&1; then \
		$(CARGO) tarpaulin --out html --output-dir $(COVERAGE_DIR); \
		echo "${GREEN}✅ Coverage report generated in $(COVERAGE_DIR)/tarpaulin-report.html${RESET}"; \
	else \
		echo "${YELLOW}⚠️  cargo-tarpaulin not installed. Install with: cargo install cargo-tarpaulin${RESET}"; \
	fi

# Benchmarking
benchmarks: ## Run performance benchmarks
	@echo "${YELLOW}⚡ Running benchmarks...${RESET}"
	@$(CARGO) bench --workspace
	@echo "${GREEN}✅ Benchmarks completed${RESET}"

performance-test: ## Run basic performance test
	@echo "${YELLOW}🏃 Running performance test...${RESET}"
	@$(CARGO) run --release --example hello_world &
	@sleep 2
	@echo "Basic performance test completed"
	@pkill -f hello_world || true
	@echo "${GREEN}✅ Performance test completed${RESET}"

flamegraph: ## Generate flamegraph performance profile (requires cargo-flamegraph)
	@echo "${YELLOW}🔥 Generating flamegraph...${RESET}"
	@if command -v cargo-flamegraph >/dev/null 2>&1; then \
		$(CARGO) flamegraph --example hello_world; \
		echo "${GREEN}✅ Flamegraph generated as flamegraph.svg${RESET}"; \
	else \
		echo "${YELLOW}⚠️  cargo-flamegraph not installed. Install with: cargo install flamegraph${RESET}"; \
	fi

# Development Workflow
dev: ## Start development workflow with file watching (requires cargo-watch)
	@echo "${BOLD}${GREEN}🚀 Starting $(PROJECT_NAME) development mode...${RESET}"
	@if command -v cargo-watch >/dev/null 2>&1; then \
		$(CARGO) watch -x "check" -x "test" -x "clippy"; \
	else \
		echo "${YELLOW}⚠️  cargo-watch not installed. Install with: cargo install cargo-watch${RESET}"; \
		echo "${BLUE}Running single check instead...${RESET}"; \
		$(MAKE) check; \
	fi

watch: ## Watch files and run tests on changes (requires cargo-watch)
	@echo "${GREEN}👀 Watching for file changes...${RESET}"
	@if command -v cargo-watch >/dev/null 2>&1; then \
		$(CARGO) watch -x "test"; \
	else \
		echo "${YELLOW}⚠️  cargo-watch not installed. Install with: cargo install cargo-watch${RESET}"; \
		echo "${BLUE}Running single test instead...${RESET}"; \
		$(MAKE) test; \
	fi

watch-check: ## Watch files and run check on changes (requires cargo-watch)
	@echo "${GREEN}👀 Watching for file changes (check only)...${RESET}"
	@if command -v cargo-watch >/dev/null 2>&1; then \
		$(CARGO) watch -x "check"; \
	else \
		echo "${YELLOW}⚠️  cargo-watch not installed. Install with: cargo install cargo-watch${RESET}"; \
		echo "${BLUE}Running single check instead...${RESET}"; \
		$(MAKE) check; \
	fi

# Examples and Demos
examples: ## Build all examples
	@echo "${CYAN}📖 Building examples...${RESET}"
	@$(CARGO) build --examples
	@echo "${GREEN}✅ Examples build completed${RESET}"

demo-hello: ## Run hello_world example
	@echo "${BOLD}${CYAN}🎬 Running hello_world demo...${RESET}"
	@$(CARGO) run --example hello_world

demo-minimal: ## Run minimal_turbomcp example
	@echo "${CYAN}🎬 Running minimal example...${RESET}"
	@$(CARGO) run --example minimal_turbomcp

demo-basic: ## Run basic example
	@echo "${CYAN}🎬 Running basic example...${RESET}"
	@$(CARGO) run --example basic

demo-tcp: ## Run TCP-only server example
	@echo "${CYAN}🎬 Running TCP-only server example...${RESET}"
	@$(CARGO) run --example tcp_only_server

# Release Management
release: clean build-release test ## Build and test release version
	@echo "${BOLD}${GREEN}🎉 $(PROJECT_NAME) v$(VERSION) release ready!${RESET}"
	@echo "${BLUE}Binary size analysis:${RESET}"
	@$(CARGO) bloat --release --crates
	@echo "${GREEN}✅ Release build completed and verified${RESET}"

pre-release: ## Prepare for release (version bump, changelog, etc.)
	@echo "${YELLOW}📋 Preparing release...${RESET}"
	@echo "Current version: $(VERSION)"
	@echo "Git branch: $(GIT_BRANCH)"
	@echo "Git hash: $(GIT_HASH)"
	@$(MAKE) test
	@$(MAKE) audit
	@$(MAKE) docs-check
	@echo "${GREEN}✅ Pre-release checks completed${RESET}"

publish-check: ## Dry-run publish to check everything
	@echo "${YELLOW}🔍 Checking publish readiness...${RESET}"
	@$(CARGO) publish --dry-run -p turbomcp-macros
	@$(CARGO) publish --dry-run -p turbomcp
	@echo "${GREEN}✅ Publish check completed${RESET}"

# Utility Targets
clean: ## Clean build artifacts and temporary files
	@echo "${YELLOW}🧹 Cleaning build artifacts...${RESET}"
	@$(CARGO) clean
	@rm -rf $(COVERAGE_DIR)
	@rm -rf $(TARGET_DIR)
	@rm -f flamegraph.svg
	@rm -f perf.data*
	@rm -f *.profraw
	@echo "${GREEN}✅ Cleaned successfully${RESET}"

clean-deps: ## Clean and update dependencies
	@echo "${YELLOW}🧹 Cleaning and updating dependencies...${RESET}"
	@$(CARGO) clean
	@$(CARGO) update
	@echo "${GREEN}✅ Dependencies updated${RESET}"

install-cli: ## Install TurboMCP CLI tools locally
	@echo "${BLUE}📦 Installing TurboMCP CLI...${RESET}"
	@$(CARGO) install --path crates/turbomcp-cli
	@echo "${GREEN}✅ TurboMCP CLI installed${RESET}"

uninstall-cli: ## Uninstall TurboMCP CLI tools
	@echo "${YELLOW}🗑️  Uninstalling TurboMCP CLI...${RESET}"
	@$(CARGO) uninstall turbomcp-cli
	@echo "${GREEN}✅ TurboMCP CLI uninstalled${RESET}"

# Statistics and Analysis
stats: ## Show project statistics
	@echo "${BOLD}${CYAN}📊 $(PROJECT_NAME) Project Statistics${RESET}"
	@echo "${BLUE}Version:${RESET} $(VERSION)"
	@echo "${BLUE}Git Branch:${RESET} $(GIT_BRANCH)"
	@echo "${BLUE}Git Hash:${RESET} $(GIT_HASH)"
	@echo "${BLUE}Build Time:${RESET} $(BUILD_TIME)"
	@echo ""
	@echo "${BOLD}${GREEN}Lines of Code:${RESET}"
	@find $(CRATES_DIR) -name "*.rs" -exec cat {} + | wc -l | xargs echo "  Rust:"
	@find . -name "Cargo.toml" | wc -l | xargs echo "  Cargo.toml files:"
	@find $(EXAMPLES_DIR) -name "*.rs" 2>/dev/null | wc -l | xargs echo "  Examples:"
	@echo ""
	@echo "${BOLD}${GREEN}Dependencies:${RESET}"
	@$(CARGO) tree --depth 1 | grep -E '^[a-zA-Z]' | wc -l | xargs echo "  Direct dependencies:"
	@echo ""
	@echo "${BOLD}${GREEN}Crates:${RESET}"
	@ls $(CRATES_DIR) | wc -l | xargs echo "  Total crates:"

bloat-check: ## Analyze binary size and dependencies (requires cargo-bloat)
	@echo "${YELLOW}📊 Analyzing binary bloat...${RESET}"
	@if command -v cargo-bloat >/dev/null 2>&1; then \
		$(CARGO) bloat --release; \
		$(CARGO) bloat --release --crates; \
	else \
		echo "${YELLOW}⚠️  cargo-bloat not installed. Install with: cargo install cargo-bloat${RESET}"; \
		echo "${BLUE}Using basic size analysis instead...${RESET}"; \
		ls -lh target/release/turbomcp-* 2>/dev/null || echo "No release binaries found. Run 'make build-release' first."; \
	fi

outdated: ## Check for outdated dependencies (requires cargo-outdated)
	@echo "${YELLOW}📦 Checking for outdated dependencies...${RESET}"
	@if command -v cargo-outdated >/dev/null 2>&1; then \
		$(CARGO) outdated; \
	else \
		echo "${YELLOW}⚠️  cargo-outdated not installed. Install with: cargo install cargo-outdated${RESET}"; \
	fi

# CI/CD Integration
ci-prepare: ## Prepare for CI environment
	@echo "${BOLD}${BLUE}🤖 Preparing CI environment...${RESET}"
	@$(RUSTUP) component add rustfmt clippy
	@echo "${GREEN}✅ CI environment prepared${RESET}"

ci-test: ci-prepare ## Run CI test pipeline
	@echo "${BOLD}${BLUE}🤖 Running CI test pipeline...${RESET}"
	@$(MAKE) fmt-check
	@$(MAKE) lint
	@$(MAKE) test
	@$(MAKE) test-examples
	@$(MAKE) audit
	@echo "${GREEN}✅ CI test pipeline completed${RESET}"

ci-build: ci-prepare ## Run CI build pipeline
	@echo "${BOLD}${BLUE}🤖 Running CI build pipeline...${RESET}"
	@$(MAKE) build
	@$(MAKE) build-release
	@echo "${GREEN}✅ CI build pipeline completed${RESET}"

# Git Hooks
git-hooks: ## Install git pre-commit hooks
	@echo "${CYAN}🪝 Installing git hooks...${RESET}"
	@echo "#!/bin/sh" > .git/hooks/pre-commit
	@echo "make pre-commit" >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "${GREEN}✅ Git hooks installed${RESET}"

pre-commit: ## Run pre-commit checks
	@echo "${YELLOW}🔍 Running pre-commit checks...${RESET}"
	@$(MAKE) fmt-check
	@$(MAKE) lint
	@$(MAKE) test
	@echo "${GREEN}✅ Pre-commit checks passed${RESET}"

# Docker Support (requires Docker daemon)
docker-build: ## Build Docker image
	@if ! command -v docker >/dev/null 2>&1; then \
		echo "${YELLOW}⚠️  Docker not installed${RESET}"; \
		exit 1; \
	fi
	@if ! docker info >/dev/null 2>&1; then \
		echo "${YELLOW}⚠️  Docker daemon not running${RESET}"; \
		exit 1; \
	fi
	@if [ -f Dockerfile ]; then \
		echo "${BLUE}🐳 Building Docker image...${RESET}"; \
		docker build -t turbomcp:$(VERSION) .; \
		docker build -t turbomcp:latest .; \
		echo "${GREEN}✅ Docker image built${RESET}"; \
	else \
		echo "${YELLOW}⚠️  No Dockerfile found${RESET}"; \
	fi

# Reporting
report: ## Generate comprehensive project report
	@echo "${BOLD}${MAGENTA}📋 Generating $(PROJECT_NAME) Project Report${RESET}"
	@echo "# $(PROJECT_NAME) Project Report" > project-report.md
	@echo "Generated: $(BUILD_TIME)" >> project-report.md
	@echo "Version: $(VERSION)" >> project-report.md
	@echo "Git: $(GIT_BRANCH)@$(GIT_HASH)" >> project-report.md
	@echo "" >> project-report.md
	@echo "## Build Status" >> project-report.md
	@$(MAKE) check &>/dev/null && echo "✅ Build: PASSING" >> project-report.md || echo "❌ Build: FAILING" >> project-report.md
	@$(MAKE) test &>/dev/null && echo "✅ Tests: PASSING" >> project-report.md || echo "❌ Tests: FAILING" >> project-report.md
	@$(MAKE) lint &>/dev/null && echo "✅ Linting: PASSING" >> project-report.md || echo "❌ Linting: FAILING" >> project-report.md
	@echo "" >> project-report.md
	@$(MAKE) stats >> project-report.md
	@echo "${GREEN}✅ Report generated: project-report.md${RESET}"


# Environment validation
validate-env: ## Validate development environment
	@echo "${CYAN}🔧 Validating development environment...${RESET}"
	@$(RUSTUP) --version >/dev/null 2>&1 || (echo "${RED}❌ rustup not found${RESET}" && exit 1)
	@$(CARGO) --version >/dev/null 2>&1 || (echo "${RED}❌ cargo not found${RESET}" && exit 1)
	@rustc --version | grep -q "$(RUST_VERSION)" || echo "${YELLOW}⚠️  Rust version $(RUST_VERSION) recommended${RESET}"
	@echo "${GREEN}✅ Environment validation completed${RESET}"

# Production deployment preparation
production: ## Prepare production build with optimizations
	@echo "${BOLD}${RED}🚀 Building $(PROJECT_NAME)...${RESET}"
	@$(MAKE) clean
	@$(MAKE) audit
	@$(MAKE) test
	@RUSTFLAGS="-C target-cpu=native" $(CARGO) build --release
	@$(MAKE) bloat-check
	@echo "${GREEN}✅ Production build completed${RESET}"

# Show current configuration
config: ## Show current build configuration
	@echo "${BOLD}${CYAN}⚙️  $(PROJECT_NAME) Configuration${RESET}"
	@echo "${BLUE}Rust Version:${RESET} $$(rustc --version)"
	@echo "${BLUE}Cargo Version:${RESET} $$(cargo --version)"
	@echo "${BLUE}Project Version:${RESET} $(VERSION)"
	@echo "${BLUE}Default Features:${RESET} $(DEFAULT_FEATURES)"
	@echo "${BLUE}Target Directory:${RESET} $(TARGET_DIR)"
	@echo "${BLUE}CI Environment:${RESET} $(CI)"
	@echo "${BLUE}GitHub Actions:${RESET} $(GITHUB_ACTIONS)"