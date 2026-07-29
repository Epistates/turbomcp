#!/usr/bin/env -S just --justfile

# TurboMCP - Production Rust MCP Framework
# =========================================
# Professional development workflow automation

# Project configuration
project_name := "TurboMCP"
rust_version := "1.89.0"

# Build flags
release_flags := "--release"
all_features_flags := "--all-features"
workspace_flags := "--workspace"

# Directories
crates_dir := "crates"
target_dir := "target"
coverage_dir := "coverage"

# Coverage: keep these in step with the `coverage` job in .github/workflows/test.yml.
# Excluded are the things line coverage cannot speak to — `@generated` wire types
# (emitted Default/From/Display impls the conversions never call; proven instead by
# the conformance suite and round-trip tests) and turbomcp-codegen, a build tool.
coverage_ignore := '(tests?/|benches/|examples/|fuzz/|/(v2025_06_18|v2025_11_25|draft)/types\.rs|turbomcp-codegen/)'
coverage_min := "85"

# v4 codegen: root of the checked-out MCP schema (override with MCP_SCHEMA_ROOT)
mcp_schema_root := env_var_or_default("MCP_SCHEMA_ROOT", "../reference/modelcontextprotocol/schema")

# Set shell for both unix and Windows environments
set shell := ["sh", "-euc"]
set windows-shell := ["sh", "-euc", "--"] # Requires Git to be installed with `sh` in PATH if on Windows

# Version info (computed)
version := `grep '^version' crates/turbomcp/Cargo.toml | head -1 | cut -d '"' -f 2`
git_hash := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`
git_branch := `git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown"`

# Aliases
alias t := test
alias b := build
alias c := check
alias f := fmt

# Default recipe - show help
default:
  @just --list --unsorted

# =============================================================================
# v4 codegen
# =============================================================================

# Regenerate the v4 per-version wire types from the MCP schema (checked in).
[group: 'v4']
codegen:
  #!/usr/bin/env bash
  set -euo pipefail
  root="{{mcp_schema_root}}"
  echo "Generating v4 protocol types from ${root}"
  cargo run -q -p turbomcp-codegen -- \
    "${root}/2025-06-18/schema.json" \
    crates/turbomcp-protocol/src/v2025_06_18/types.rs "MCP 2025-06-18"
  cargo run -q -p turbomcp-codegen -- \
    "${root}/2025-11-25/schema.json" \
    crates/turbomcp-protocol/src/v2025_11_25/types.rs "MCP 2025-11-25"
  cargo run -q -p turbomcp-codegen -- \
    "${root}/draft/schema.json" \
    crates/turbomcp-protocol/src/draft/types.rs "MCP 2026-07-28"
  cargo fmt -p turbomcp-protocol
  echo "Done. Review the diff before committing."

# =============================================================================
# Setup
# =============================================================================

# Set up development environment
[group: 'setup']
setup:
  #!/usr/bin/env bash
  set -euo pipefail
  echo "Setting up {{project_name}} development environment..."
  rustup toolchain install {{rust_version}}
  rustup default {{rust_version}}
  rustup component add rustfmt clippy llvm-tools-preview
  echo "Development environment ready!"

# Install optional development tools
[group: 'setup']
setup-tools:
  #!/usr/bin/env bash
  set -euo pipefail
  echo "Installing optional development tools..."
  echo "Installing core tools..."
  cargo install cargo-watch || echo "Failed to install cargo-watch"
  cargo install cargo-llvm-cov || echo "Failed to install cargo-llvm-cov"
  echo "Installing analysis tools..."
  cargo install cargo-audit || echo "Failed to install cargo-audit"
  cargo install cargo-outdated || echo "Failed to install cargo-outdated"
  cargo install cargo-bloat || echo "Failed to install cargo-bloat"
  echo "Installing performance tools..."
  cargo install cargo-tarpaulin || echo "Failed to install cargo-tarpaulin"
  cargo install flamegraph || echo "Failed to install flamegraph"
  echo "Tool installation completed (some may have failed)"

# Show status of optional development tools
[group: 'setup']
tool-status:
  #!/usr/bin/env bash
  echo "Development Tool Status"
  echo "Core Tools:"
  command -v cargo-watch >/dev/null 2>&1 && echo "  cargo-watch" || echo "  cargo-watch (install: cargo install cargo-watch)"
  command -v cargo-llvm-cov >/dev/null 2>&1 && echo "  cargo-llvm-cov" || echo "  cargo-llvm-cov (install: cargo install cargo-llvm-cov)"
  echo "Analysis Tools:"
  command -v cargo-audit >/dev/null 2>&1 && echo "  cargo-audit" || echo "  cargo-audit (install: cargo install cargo-audit)"
  command -v cargo-outdated >/dev/null 2>&1 && echo "  cargo-outdated" || echo "  cargo-outdated (install: cargo install cargo-outdated)"
  command -v cargo-bloat >/dev/null 2>&1 && echo "  cargo-bloat" || echo "  cargo-bloat (install: cargo install cargo-bloat)"
  echo "Performance Tools:"
  command -v cargo-tarpaulin >/dev/null 2>&1 && echo "  cargo-tarpaulin" || echo "  cargo-tarpaulin (install: cargo install cargo-tarpaulin)"
  command -v cargo-flamegraph >/dev/null 2>&1 && echo "  cargo-flamegraph" || echo "  cargo-flamegraph (install: cargo install flamegraph)"
  echo "System Tools:"
  command -v docker >/dev/null 2>&1 && echo "  docker" || echo "  docker"

# Validate development environment
[group: 'setup']
validate-env:
  #!/usr/bin/env bash
  set -euo pipefail
  echo "Validating development environment..."
  rustup --version >/dev/null 2>&1 || (echo "rustup not found" && exit 1)
  cargo --version >/dev/null 2>&1 || (echo "cargo not found" && exit 1)
  rustc --version | grep -q "{{rust_version}}" || echo "Rust version {{rust_version}} recommended"
  echo "Environment validation completed"

# =============================================================================
# Build
# =============================================================================

# Build all crates in development mode
[group: 'build']
build:
  @echo "Building {{project_name}}..."
  cargo build {{workspace_flags}}
  @echo "Build completed successfully"

# Build optimized release version
[group: 'build']
build-release:
  @echo "Building {{project_name}} release..."
  cargo build {{workspace_flags}} {{release_flags}}
  @echo "Release build completed"

# Build with all features enabled
[group: 'build']
build-all-features:
  @echo "Building {{project_name}} with all features..."
  cargo build {{workspace_flags}} {{all_features_flags}}
  @echo "All features build completed"

# =============================================================================
# Test
# =============================================================================

# Run comprehensive test suite (tests + clippy + fmt)
[group: 'test']
test:
  echo "Running comprehensive test suite..."
  echo "Step 1/6: Running unit, integration, and doc tests (all features)..."
  cargo test --workspace --all-features
  echo "Step 2/6: Running clippy on all crates, targets, and examples..."
  cargo clippy {{workspace_flags}} --all-targets --all-features -- -D warnings
  echo "Step 3/6: Verifying the no-default-features facade still lints..."
  cargo clippy -p turbomcp -- -D warnings
  echo "Step 4/6: Testing non-default foundation configs (no_std core/protocol, no-simd codec)..."
  cargo test -p turbomcp-core -p turbomcp-protocol -p turbomcp-codec --no-default-features
  echo "Step 5/7: Checking formatting on all code..."
  cargo fmt --all -- --check
  echo "Step 6/7: Verifying wasm portability (no_std foundation, default + no-default)..."
  cargo build -p turbomcp-core -p turbomcp-protocol --target wasm32-unknown-unknown
  cargo build -p turbomcp-core -p turbomcp-protocol -p turbomcp-codec --no-default-features --target wasm32-unknown-unknown
  echo "Step 7/7: Building docs the way docs.rs does (nightly, --cfg docsrs)..."
  just docs-rs
  echo "All tests, linting, and formatting checks passed!"

# Run tests only (no linting/formatting)
[group: 'test']
test-only:
  @echo "Running tests only..."
  cargo test {{workspace_flags}} --lib --tests
  @echo "All tests passed"

# Run tests with all features enabled
[group: 'test']
test-all-features:
  @echo "Running tests with all features..."
  cargo test {{workspace_flags}} {{all_features_flags}} --lib --tests
  @echo "All features tests passed"

# Run unit tests only
[group: 'test']
test-unit:
  @echo "Running unit tests..."
  cargo test {{workspace_flags}} --lib

# Run comprehensive integration tests only
[group: 'test']
test-integration:
  @echo "Running integration tests..."
  cargo test --package turbomcp --test integration_tests
  @echo "Integration tests passed!"

# Run all integration tests in workspace
[group: 'test']
test-integration-all:
  @echo "Running all integration tests..."
  cargo test {{workspace_flags}} --tests

# Run zero-tolerance test quality enforcement
[group: 'test']
test-enforce:
  @echo "Running zero-tolerance test quality enforcement..."
  cargo test --package turbomcp --test v3_audit
  @echo "Zero-tolerance enforcement passed!"

# Run all tests including zero-tolerance enforcement
[group: 'test']
test-all: test test-enforce
  @echo "All tests and enforcement checks passed!"

# Test documentation examples
[group: 'test']
test-docs:
  @echo "Testing documentation examples..."
  cargo test {{workspace_flags}} --doc

# Build and test all examples
[group: 'test']
test-examples:
  @echo "Building examples..."
  cargo build --examples
  @echo "Examples build completed"

# Run tests matching a pattern
[group: 'test']
filter PATTERN:
  cargo test {{PATTERN}} -- --nocapture

# =============================================================================
# Code Quality
# =============================================================================

# Format code using rustfmt
[group: 'quality']
fmt:
  @echo "Formatting code..."
  cargo fmt --all
  @echo "Code formatting completed"

# Check code formatting without making changes
[group: 'quality']
fmt-check:
  @echo "Checking code formatting..."
  cargo fmt --all -- --check

# Run clippy linter
[group: 'quality']
lint:
  @echo "Linting code..."
  cargo clippy {{workspace_flags}} --all-targets -- -D warnings
  @echo "Linting completed"

# Auto-fix clippy warnings where possible
[group: 'quality']
lint-fix:
  @echo "Auto-fixing lint issues..."
  cargo clippy {{workspace_flags}} --all-targets --fix --allow-dirty

# Fast compile check without building
[group: 'quality']
check:
  @echo "Running fast check..."
  cargo check {{workspace_flags}} --all-targets

# Check with all features enabled
[group: 'quality']
check-all-features:
  @echo "Checking with all features..."
  cargo check {{workspace_flags}} {{all_features_flags}} --all-targets

# Check dependency tree
[group: 'quality']
check-deps:
  @echo "Checking dependencies..."
  cargo tree

# =============================================================================
# Security & Audit
# =============================================================================

# Check the public API against the last published 4.x for SemVer breakage.
# No-op until a 4.x is on crates.io (there is nothing to compare against yet).
[group: 'quality']
semver-check:
  #!/usr/bin/env bash
  set -euo pipefail
  if ! command -v cargo-semver-checks >/dev/null 2>&1; then
    echo "cargo-semver-checks not installed. Install with: cargo install cargo-semver-checks"
    exit 0
  fi
  baseline=$(curl -sSf -H 'User-Agent: turbomcp (nick@epistates.com)' \
    https://crates.io/api/v1/crates/turbomcp \
    | jq -r '[.versions[] | select(.yanked == false) | .num
             | select(startswith("4."))] | first // empty')
  if [ -z "$baseline" ]; then
    echo "No published 4.x baseline yet — nothing to compare against."
    exit 0
  fi
  echo "Comparing the public API against turbomcp $baseline"
  cargo semver-checks check-release --package turbomcp --baseline-version "$baseline"

# Security audit of dependencies
[group: 'security']
audit:
  #!/usr/bin/env bash
  echo "Running security audit..."
  if command -v cargo-audit >/dev/null 2>&1; then
    cargo audit
    echo "Security audit completed"
  else
    echo "cargo-audit not installed. Install with: cargo install cargo-audit"
  fi

# Fuzz every untrusted-input decoder briefly (needs nightly + cargo-fuzz)
[group: 'security']
fuzz secs='30':
  #!/usr/bin/env bash
  set -euo pipefail
  # The bounded regression run that used to be a CI job: each target must build
  # under the sanitizer and survive `secs` without a crash. A guard, not a
  # campaign — see `fuzz-long`. Setup:
  #   rustup toolchain install nightly && cargo install cargo-fuzz
  #
  # cargo-fuzz defaults --target to the triple it was itself compiled for, which
  # for the prebuilt musl binary means building targets for musl, where ASan is
  # unsupported. Pin to the actual host triple.
  host="$(rustc +nightly -vV | sed -n 's/^host: //p')"
  cd fuzz
  for target in codec_decode mcp_header_codec uri_template sonic_decode; do
    echo "==> fuzzing $target for {{secs}}s"
    cargo +nightly fuzz run "$target" --target "$host" -- \
      -max_total_time={{secs}} -rss_limit_mb=4096
  done
  echo "All fuzz targets survived {{secs}}s each."

# A real campaign against one target. `just fuzz-long codec_decode 3600`.
[group: 'security']
fuzz-long target secs='3600':
  #!/usr/bin/env bash
  set -euo pipefail
  host="$(rustc +nightly -vV | sed -n 's/^host: //p')"
  cd fuzz
  cargo +nightly fuzz run "{{target}}" --target "$host" -- \
    -max_total_time={{secs}} -rss_limit_mb=4096

# Comprehensive security analysis
[group: 'security']
security: audit

# =============================================================================
# Documentation
# =============================================================================

# Generate and open documentation
[group: 'docs']
docs:
  @echo "Generating documentation..."
  cargo doc --workspace --no-deps --open
  @echo "Documentation generated"

# Build documentation without opening
[group: 'docs']
docs-build:
  @echo "Building documentation..."
  cargo doc --workspace --no-deps

# Check documentation for broken links and issues
[group: 'docs']
docs-check: test-docs
  @echo "Checking documentation..."
  cargo doc --workspace --no-deps --document-private-items

# Build docs exactly as docs.rs does, failing on any warning (needs nightly)
[group: 'docs']
docs-rs:
  #!/usr/bin/env bash
  set -euo pipefail
  # nightly + `--cfg docsrs` so the `doc(cfg(feature = ...))` labels compile.
  #
  # This is the ONLY enforcement of rustdoc correctness — there is no CI job,
  # because docs.rs's configuration needs nightly and CI is stable-only. A broken
  # doc build is invisible until it renders wrong on docs.rs, so `just test` runs
  # this as its last step.
  #
  # turbomcp-protocol is exempt: its @generated types embed the spec's prose,
  # which rustdoc misreads as code (same reason it opts out of doctests).
  if ! rustup toolchain list | grep -q '^nightly'; then
    echo "error: this check needs a nightly toolchain (docs.rs builds on one)." >&2
    echo "       rustup toolchain install nightly" >&2
    exit 1
  fi
  echo "Building docs in the docs.rs configuration..."
  RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc \
    --workspace --all-features --no-deps --exclude turbomcp-protocol
  echo "Rustdoc clean"

# =============================================================================
# Coverage
# =============================================================================

# Generate test coverage report
[group: 'coverage']
coverage:
  #!/usr/bin/env bash
  echo "Generating coverage report..."
  if command -v cargo-llvm-cov >/dev/null 2>&1; then
    cargo llvm-cov --html --output-dir {{coverage_dir}} {{workspace_flags}} \
      {{all_features_flags}} --all-targets --ignore-filename-regex '{{coverage_ignore}}'
    echo "Coverage report generated in {{coverage_dir}}/index.html"
  else
    echo "cargo-llvm-cov not installed. Install with: cargo install cargo-llvm-cov"
  fi

# Show the coverage summary, and fail below the floor CI enforces
[group: 'coverage']
coverage-text:
  #!/usr/bin/env bash
  set -euo pipefail
  if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "cargo-llvm-cov not installed. Install with: cargo install cargo-llvm-cov"
    exit 1
  fi
  cargo llvm-cov {{workspace_flags}} {{all_features_flags}} --all-targets \
    --summary-only --ignore-filename-regex '{{coverage_ignore}}'
  COVERAGE=$(cargo llvm-cov report --json --summary-only \
    --ignore-filename-regex '{{coverage_ignore}}' | jq -r '.data[0].totals.lines.percent')
  if ! printf '%s' "$COVERAGE" | grep -Eq '^[0-9]+(\.[0-9]+)?$'; then
    echo "could not parse a coverage percentage (got '${COVERAGE}')" >&2
    exit 1
  fi
  printf 'Line coverage: %.2f%% (floor %s%%)\n' "$COVERAGE" '{{coverage_min}}'
  if awk -v c="$COVERAGE" -v m='{{coverage_min}}' 'BEGIN { exit !(c < m) }'; then
    echo "Coverage is below the {{coverage_min}}% floor CI enforces" >&2
    exit 1
  fi

# Generate coverage using tarpaulin
[group: 'coverage']
coverage-tarpaulin:
  #!/usr/bin/env bash
  echo "Generating coverage with tarpaulin..."
  if command -v cargo-tarpaulin >/dev/null 2>&1; then
    cargo tarpaulin --out html --output-dir {{coverage_dir}}
    echo "Coverage report generated in {{coverage_dir}}/tarpaulin-report.html"
  else
    echo "cargo-tarpaulin not installed. Install with: cargo install cargo-tarpaulin"
  fi

# =============================================================================
# Benchmarking
# =============================================================================

# Run performance benchmarks
[group: 'bench']
benchmarks:
  @echo "Running benchmarks..."
  cargo bench --workspace
  @echo "Benchmarks completed"

# Run basic performance test
[group: 'bench']
performance-test:
  #!/usr/bin/env bash
  echo "Running performance test..."
  cargo run --release --example hello_world &
  sleep 2
  echo "Basic performance test completed"
  pkill -f hello_world || true
  echo "Performance test completed"

# Generate flamegraph performance profile
[group: 'bench']
flamegraph:
  #!/usr/bin/env bash
  echo "Generating flamegraph..."
  if command -v cargo-flamegraph >/dev/null 2>&1; then
    cargo flamegraph --example hello_world
    echo "Flamegraph generated as flamegraph.svg"
  else
    echo "cargo-flamegraph not installed. Install with: cargo install flamegraph"
  fi

# =============================================================================
# Development Workflow
# =============================================================================

# Start development workflow with file watching
[group: 'dev']
dev:
  #!/usr/bin/env bash
  echo "Starting {{project_name}} development mode..."
  if command -v cargo-watch >/dev/null 2>&1; then
    cargo watch -x "check" -x "test" -x "clippy"
  else
    echo "cargo-watch not installed. Install with: cargo install cargo-watch"
    echo "Running single check instead..."
    just check
  fi

# Watch files and run tests on changes
[group: 'dev']
watch:
  #!/usr/bin/env bash
  echo "Watching for file changes..."
  if command -v cargo-watch >/dev/null 2>&1; then
    cargo watch -x "test"
  else
    echo "cargo-watch not installed. Install with: cargo install cargo-watch"
    echo "Running single test instead..."
    just test
  fi

# Watch files and run check on changes
[group: 'dev']
watch-check:
  #!/usr/bin/env bash
  echo "Watching for file changes (check only)..."
  if command -v cargo-watch >/dev/null 2>&1; then
    cargo watch -x "check"
  else
    echo "cargo-watch not installed. Install with: cargo install cargo-watch"
    echo "Running single check instead..."
    just check
  fi

# =============================================================================
# Examples and Demos
# =============================================================================

# Build all examples
[group: 'examples']
examples:
  @echo "Building examples..."
  cargo build --examples
  @echo "Examples build completed"

# Run hello_world example
[group: 'examples']
demo-hello:
  @echo "Running hello_world demo..."
  cargo run --example hello_world

# Run minimal_turbomcp example
[group: 'examples']
demo-minimal:
  @echo "Running minimal example..."
  cargo run --example minimal_turbomcp

# Run basic example
[group: 'examples']
demo-basic:
  @echo "Running basic example..."
  cargo run --example basic

# Run TCP-only server example
[group: 'examples']
demo-tcp:
  @echo "Running TCP-only server example..."
  cargo run --example tcp_only_server

# =============================================================================
# Release Management
# =============================================================================

# Build and test release version
[group: 'release']
release: clean build-release test
  #!/usr/bin/env bash
  echo "{{project_name}} v{{version}} release ready!"
  echo "Binary size analysis:"
  cargo bloat --release --crates || echo "cargo-bloat not installed"
  echo "Release build completed and verified"

# Prepare for release (version bump, changelog, etc.)
[group: 'release']
pre-release: test audit docs-check
  #!/usr/bin/env bash
  echo "Preparing release..."
  echo "Current version: {{version}}"
  echo "Git branch: {{git_branch}}"
  echo "Git hash: {{git_hash}}"
  echo "Pre-release checks completed"

# Print the order the crates must be published in
[group: 'release']
publish-order:
  #!/usr/bin/env bash
  set -euo pipefail
  # Derived from the real dependency graph rather than a hand-kept list.
  # crates.io resolves a dependency the moment it is published, so a crate
  # published before something it depends on is rejected — and a release cannot
  # be undone, only yanked, which leaves a partial release on the index forever.
  cargo metadata --no-deps --format-version 1 | jq -r '
    .packages[] | select(.publish != []) | .name as $n
    | ([.dependencies[].name | select(startswith("turbomcp"))] | unique) as $deps
    | if ($deps | length) == 0 then "\($n) \($n)" else ($deps[] | "\(.) \($n)") end
  ' | tsort

# Check version consistency, crates.io metadata, and packaged file lists
[group: 'release']
publish-check:
  #!/usr/bin/env bash
  set -euo pipefail
  # Note what is *not* here: building each packaged tarball. Both `cargo publish
  # --dry-run` and `cargo package` resolve dependencies against crates.io, where
  # a packaged crate's path deps have become registry deps — so on the first
  # release of a version every crate but the graph roots fails, because its
  # siblings do not exist on the index yet. That is a property of publishing a
  # workspace, not something to work around, and it is exactly why `just
  # publish` goes in dependency order.
  #
  # What is checkable without resolution: one shared version, the metadata
  # crates.io requires, and the file list each crate would ship.

  echo "== versions =="
  versions=$(cargo metadata --no-deps --format-version 1 \
    | jq -r '[.packages[] | select(.publish != []) | .version] | unique | .[]')
  echo "$versions" | sed 's/^/  /'
  if [ "$(echo "$versions" | wc -l | tr -d ' ')" != "1" ]; then
    echo "  ERROR: publishable crates disagree on the version" >&2
    exit 1
  fi

  echo
  echo "== required metadata =="
  missing=$(cargo metadata --no-deps --format-version 1 | jq -r '
    .packages[] | select(.publish != [])
    | . as $p
    | [ (if $p.description then empty else "description" end)
      , (if $p.license then empty else "license" end)
      , (if $p.repository then empty else "repository" end)
      , (if $p.readme then empty else "readme" end)
      ] as $gaps
    | if ($gaps | length) > 0 then "\($p.name): missing \($gaps | join(", "))" else empty end')
  if [ -n "$missing" ]; then
    echo "$missing" | sed 's/^/  /' >&2
    exit 1
  fi
  echo "  all crates carry description, license, repository, readme"

  echo
  echo "== packaged file counts =="
  for crate in $(just publish-order); do
    n=$(cargo package --list -p "$crate" --allow-dirty 2>/dev/null | wc -l | tr -d ' ')
    printf '  %-26s %s files\n' "$crate" "$n"
  done

  echo
  echo "Publish order:"
  just publish-order | sed 's/^/  /'

# Publish every crate to crates.io in dependency order (needs CONFIRM=yes)
[group: 'release']
publish:
  #!/usr/bin/env bash
  set -euo pipefail
  # Guarded because it cannot be undone: crates.io allows yanking, not deletion.
  order=$(just publish-order)
  if [ "${CONFIRM:-}" != "yes" ]; then
    echo "Would publish {{version}} in this order:"
    echo "$order" | sed 's/^/  /'
    echo
    echo "This is irreversible — crates.io allows yanking, not deletion."
    echo "Re-run with CONFIRM=yes to publish."
    exit 0
  fi
  for crate in $order; do
    echo "==> publishing $crate"
    cargo publish -p "$crate"
    # crates.io indexes asynchronously; the next crate's dependency on this one
    # is unresolvable until it lands. `cargo publish` waits for the index by
    # default, but a short settle keeps a slow index from failing the chain.
    sleep 15
  done
  echo "Published {{version}}."

# =============================================================================
# Utilities
# =============================================================================

# Clean build artifacts and temporary files
[group: 'util']
clean:
  #!/usr/bin/env bash
  echo "Cleaning build artifacts..."
  cargo clean
  rm -rf {{coverage_dir}}
  rm -rf {{target_dir}}
  rm -f flamegraph.svg
  rm -f perf.data*
  rm -f *.profraw
  echo "Cleaned successfully"

# Clean and update dependencies
[group: 'util']
clean-deps:
  @echo "Cleaning and updating dependencies..."
  cargo clean
  cargo update
  @echo "Dependencies updated"

# =============================================================================
# Statistics and Analysis
# =============================================================================

# Show project statistics
[group: 'stats']
stats:
  #!/usr/bin/env bash
  echo "{{project_name}} Project Statistics"
  echo "Version: {{version}}"
  echo "Git Branch: {{git_branch}}"
  echo "Git Hash: {{git_hash}}"
  echo ""
  echo "Lines of Code:"
  find {{crates_dir}} -name "*.rs" -exec cat {} + | wc -l | xargs echo "  Rust:"
  find . -name "Cargo.toml" | wc -l | xargs echo "  Cargo.toml files:"
  echo ""
  echo "Dependencies:"
  cargo tree --depth 1 | grep -E '^[a-zA-Z]' | wc -l | xargs echo "  Direct dependencies:"
  echo ""
  echo "Crates:"
  ls {{crates_dir}} | wc -l | xargs echo "  Total crates:"

# Analyze binary size and dependencies
[group: 'stats']
bloat-check:
  #!/usr/bin/env bash
  echo "Analyzing binary bloat..."
  if command -v cargo-bloat >/dev/null 2>&1; then
    cargo bloat --release
    cargo bloat --release --crates
  else
    echo "cargo-bloat not installed. Install with: cargo install cargo-bloat"
    echo "Using basic size analysis instead..."
    ls -lh target/release/turbomcp-* 2>/dev/null || echo "No release binaries found. Run 'just build-release' first."
  fi

# Check for outdated dependencies
[group: 'stats']
outdated:
  #!/usr/bin/env bash
  echo "Checking for outdated dependencies..."
  if command -v cargo-outdated >/dev/null 2>&1; then
    cargo outdated
  else
    echo "cargo-outdated not installed. Install with: cargo install cargo-outdated"
  fi

# Show current build configuration
[group: 'stats']
config:
  #!/usr/bin/env bash
  echo "{{project_name}} Configuration"
  echo "Rust Version: $(rustc --version)"
  echo "Cargo Version: $(cargo --version)"
  echo "Project Version: {{version}}"
  echo "Target Directory: {{target_dir}}"

# =============================================================================
# CI/CD Integration
# =============================================================================

# Prepare for CI environment
[group: 'ci']
ci-prepare:
  @echo "Preparing CI environment..."
  rustup component add rustfmt clippy
  @echo "CI environment prepared"

# Run CI test pipeline
[group: 'ci']
ci-test: ci-prepare fmt-check lint test test-examples audit
  @echo "CI test pipeline completed"

# Run CI build pipeline
[group: 'ci']
ci-build: ci-prepare build build-release
  @echo "CI build pipeline completed"

# =============================================================================
# Git Hooks
# =============================================================================

# Install git pre-commit hooks
[group: 'git']
git-hooks:
  #!/usr/bin/env bash
  echo "Installing git hooks..."
  echo "#!/bin/sh" > .git/hooks/pre-commit
  echo "just pre-commit" >> .git/hooks/pre-commit
  chmod +x .git/hooks/pre-commit
  echo "Git hooks installed"

# Run pre-commit checks
[group: 'git']
pre-commit: fmt-check lint test
  @echo "Pre-commit checks passed"

# =============================================================================
# Docker Support
# =============================================================================

# Build Docker image
[group: 'docker']
docker-build:
  #!/usr/bin/env bash
  if ! command -v docker >/dev/null 2>&1; then
    echo "Docker not installed"
    exit 1
  fi
  if ! docker info >/dev/null 2>&1; then
    echo "Docker daemon not running"
    exit 1
  fi
  if [ -f Dockerfile ]; then
    echo "Building Docker image..."
    docker build -t turbomcp:{{version}} .
    docker build -t turbomcp:latest .
    echo "Docker image built"
  else
    echo "No Dockerfile found"
  fi

# =============================================================================
# Reporting
# =============================================================================

# Generate comprehensive project report
[group: 'report']
report:
  #!/usr/bin/env bash
  echo "Generating {{project_name}} Project Report"
  {
    echo "# {{project_name}} Project Report"
    echo "Generated: $(date -u '+%Y-%m-%d_%H:%M:%S_UTC')"
    echo "Version: {{version}}"
    echo "Git: {{git_branch}}@{{git_hash}}"
    echo ""
    echo "## Build Status"
    just check &>/dev/null && echo "Build: PASSING" || echo "Build: FAILING"
    just test &>/dev/null && echo "Tests: PASSING" || echo "Tests: FAILING"
    just lint &>/dev/null && echo "Linting: PASSING" || echo "Linting: FAILING"
    echo ""
  } > project-report.md
  just stats >> project-report.md
  echo "Report generated: project-report.md"

# Local Variables:
# mode: makefile
# End:
# vim: set ft=make :
