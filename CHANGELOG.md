# Changelog

All notable changes to TurboMCP will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.4] - 2025-10-22

### Added

- **Explicit Transport Selection with `transports` Attribute**: New optional macro parameter for specifying which transports a server uses
  - Reduces generated code by only creating methods for specified transports
  - Eliminates cfg warnings on Nightly Rust when transports are specified
  - Supported values: `stdio`, `http`, `websocket`, `tcp`, `unix`
  - Example: `#[server(name = "my-server", version = "0.1.0", transports = ["stdio"])]`
  - Compile-time validation with helpful error messages
  - Fully backward compatible (omitting attribute generates all transports as before)

### Changed

- **Schema-Generation Now Unconditional**: Moved `schemars` from optional to always-enabled dependency
  - Schema generation is now available by default (required for MCP spec compliance)
  - Only affects build-time dependencies (zero runtime overhead)
  - Simplified mental model: users don't have to remember to enable schema-generation feature
  - Still works with `default-features = false` if needed

- **Macro Warnings Strategy**: Removed `#[allow(unexpected_cfgs)]` from generated code
  - Cfg warnings on Nightly Rust now provide actionable guidance
  - Guides users toward explicit transport specification
  - Cleaner design: warnings point to solutions rather than hiding issues
  - Stable Rust (1.89+) unaffected (no warnings by default)

### Fixed

- **Code Quality**: Removed anti-pattern of suppressing warnings in generated code
- **Schema Module**: Removed fallback implementations and unused cfg guards

### Technical Details

- Added transport validation in `attrs.rs`
- Conditional method generation in `bidirectional_wrapper.rs`
- Wire transport attribute through macro pipeline in `server.rs` and `compile_time_router.rs`
- Added comprehensive `examples/transports_demo.rs` showing all usage patterns

### Backward Compatibility

- ✅ Zero breaking changes
- ✅ All existing code continues to work
- ✅ Fully backward compatible with 2.0.3

## [2.0.3] - 2025-10-21

### Added

- **Configurable Concurrency Limits**: Semaphore-based concurrency is now configurable for production flexibility
  - **WebSocket Server**: `WebSocketServerConfig::max_concurrent_requests` (default: 100)
    - Configure via `WebSocketServerConfig { max_concurrent_requests: 200, .. }`
    - Limits concurrent client→server request handlers per connection
  - **Client**: `ClientCapabilities::max_concurrent_handlers` (default: 100)
    - Configure via `ClientBuilder::new().with_max_concurrent_handlers(200)`
    - Limits concurrent server→client request/notification handlers
  - **Tuning Guide**:
    - Low-resource systems: 50
    - Standard deployments: 100 (default)
    - High-performance: 200-500
    - Maximum recommended: 1000
  - **Benefits**: Production deployments can tune resource usage based on available memory/CPU

### Fixed

- **Macro-Generated Doc Test Failures**: Fixed compilation failures when users run `cargo test` on projects using the `#[server]` macro
  - **Issue**: Generated methods (`run_stdio()`, `run_tcp()`, `into_mcp_router()`, etc.) had doc examples marked as ````no_run`, which still compiles the code
  - **Root Cause**: Placeholder names like `MyServer` in examples attempted to compile in user projects, causing `cannot find value 'MyServer'` errors
  - **Solution**: Changed all macro-generated doc examples from ````no_run`/````rust,no_run` to ````rust,ignore`
  - **Files Modified**:
    - `crates/turbomcp-macros/src/bidirectional_wrapper.rs` (4 doc examples)
    - `crates/turbomcp-macros/src/compile_time_router.rs` (2 doc examples)
  - **Impact**: Users can now run `cargo test` without failures from turbomcp-generated documentation
  - **Details**: See `MACRO_DOC_TEST_FIX.md` for complete analysis

- **Task Lifecycle Management - Comprehensive Hardening**: Fixed critical "JoinHandle polled after completion" panics and implemented task lifecycle management across all transports
  - **Issue**: Spawned tasks without proper lifecycle management caused panics on clean shutdown and potential resource leaks
  - **Root Cause**: `tokio::spawn()` returned JoinHandles that were immediately dropped, leaving tasks orphaned
  - **Impact**: STDIO servers panicked on EOF, WebSocket/TCP/Client handlers could leak resources
  - **Scope**: Comprehensive fix across 4 major components
  
  #### Component 1: STDIO Transport (`turbomcp-server/src/runtime.rs`)
  - **Pattern**: JoinSet with graceful shutdown
  - **Changes**:
    - Added `use tokio::task::JoinSet` import
    - Refactored `run_stdio_bidirectional()` to track all spawned tasks in JoinSet
    - Implemented graceful shutdown with 5-second timeout and abort fallback
    - Added comprehensive unit tests (6 tests) and integration tests (9 tests)
  - **Result**: No more panics on clean EOF, all tasks properly cleaned up
  - **Tests**: `runtime::tests::*`, `stdio_lifecycle_test.rs`
  
  #### Component 2: WebSocket Server (`turbomcp-server/src/runtime/websocket.rs`)
  - **Pattern**: Semaphore for bounded concurrency (industry best practice)
  - **Changes**:
    - Added `use tokio::sync::Semaphore` import
    - Implemented semaphore-based concurrency control (configurable, default 100)
    - Per-request tasks use RAII pattern (permits auto-released on drop)
    - Main send/receive loops already properly tracked with tokio::select!
    - **NEW**: Added `max_concurrent_requests` field to `WebSocketServerConfig`
  - **Benefits**: Automatic backpressure, prevents resource exhaustion, simpler than JoinSet for short-lived tasks, **production configurable**
  - **Result**: Bounded concurrency, no resource leaks, production-ready
  
  #### Component 3: TCP Transport (`turbomcp-transport/src/tcp.rs`)
  - **Pattern**: JoinSet with shutdown signal + nested JoinSet for connections
  - **Changes**:
    - Added task tracking fields to `TcpTransport` struct
    - Implemented graceful shutdown in `disconnect()` method
    - Accept loop listens for shutdown signals via `tokio::select!`
    - Connection handlers tracked in nested JoinSet
  - **Result**: Clean shutdown of accept loop and all active connections
  - **Tests**: Existing TCP tests pass with new implementation
  
  #### Component 4: Client Handlers (`turbomcp-client/src/client/core.rs`)
  - **Pattern**: Semaphore for bounded concurrency (consistent with WebSocket)
  - **Changes**:
    - Added `handler_semaphore: Arc<Semaphore>` to `ClientInner` struct
    - Updated both constructors (`new()` and `with_capabilities()`)
    - Request and notification handlers acquire permits before processing
    - Automatic cleanup via RAII pattern
    - **NEW**: Added `max_concurrent_handlers` field to `ClientCapabilities`
    - **NEW**: Added `with_max_concurrent_handlers()` builder method
  - **Result**: Bounded concurrent request processing, prevents resource exhaustion, **production configurable**
  - **Tests**: All 72 client tests pass
  
  #### Architecture & Patterns
  - **Long-Running Infrastructure Tasks** → JoinSet + Shutdown Signal
    - Accept loops, keep-alive monitors, health checks
    - Graceful shutdown with timeout and abort fallback
    - Example: STDIO stdout writer, TCP accept loop
  - **Short-Lived Request Handlers** → Semaphore for Bounded Concurrency
    - HTTP/WebSocket/Client request handlers
    - Automatic backpressure and resource control
    - Example: WebSocket per-request spawns, client handlers
  - **Fire-and-Forget** → Explicitly Documented (rare, requires review)
    - Non-critical logging, metrics emission
    - Must be <100ms and truly non-critical
  
  #### Testing
  - **Unit Tests**: 6 new tests in `runtime::tests::*`
  - **Integration Tests**: 9 new tests in `stdio_lifecycle_test.rs`
  - **Regression Prevention**: Tests verify clean shutdown without panics
  - **All Existing Tests Pass**: No breaking changes
  
  #### Breaking Changes
  - **None** - All changes are internal implementation details
  - Public APIs unchanged
  - Backward compatible
  - Can be released as patch version (2.0.3)
  
  #### Performance Impact
  - **JoinSet Overhead**: ~16 bytes per task + Arc operations (negligible for infrastructure tasks)
  - **Semaphore Overhead**: Fixed memory, atomic operations (highly efficient)
  - **Shutdown Time**: +0-5 seconds for graceful cleanup (configurable timeout)
  - **Runtime Overhead**: None - tasks run identically
  
  #### Files Changed
  - `crates/turbomcp-server/src/runtime.rs` - STDIO JoinSet implementation
  - `crates/turbomcp-server/src/runtime/websocket.rs` - WebSocket semaphore implementation
  - `crates/turbomcp-transport/src/tcp.rs` - TCP JoinSet implementation
  - `crates/turbomcp-client/src/client/core.rs` - Client semaphore implementation
  - `crates/turbomcp-server/tests/stdio_lifecycle_test.rs` - New integration tests
  - `TASK_LIFECYCLE_GUIDELINES.md` - Developer guidelines
  - `TASK_LIFECYCLE_ANALYSIS.md` - Technical analysis
  - `TASK_LIFECYCLE_VISUAL.md` - Visual documentation
  
  #### Verification Steps
  ```bash
  # All tests pass
  cargo test --package turbomcp-server runtime::tests      # 6 tests ✅
  cargo test --package turbomcp-server stdio_lifecycle_test # 9 tests ✅  
  cargo test --package turbomcp-transport tcp              # 1 test ✅
  cargo test --package turbomcp-client                     # 72 tests ✅
  
  # Manual verification
  echo '{"jsonrpc":"2.0","method":"ping","id":1}' | cargo run --example stdio_server
  # Expected: Clean exit without panic ✅
  ```
  

## [2.0.2] - 2025-10-19

### Fixed

- **Resource Reading Broken**: Fixed critical bug where resources could be listed but not read
  - **Issue**: Resources were registered by method name but looked up by URI, causing "Resource not found" errors
  - **Root Cause**: `#[server]` macro registered resources using `resource_name` instead of `resource_uri_template` as the DashMap key
  - **Impact**: All `resources/read` requests failed with -32004 error even for valid resources
  - **Fix**: Changed registration in `turbomcp-macros/src/server.rs:436` to use URI as key
  - **Location**: `crates/turbomcp-macros/src/server.rs:436`
  - **Example**: `#[resource("stdio://help")]` now registers with key "stdio://help" not "help"
  - **Breaking Change**: No - this was a bug preventing correct MCP behavior
  - **Regression Test**: Added `test_resource_registration_lookup_by_uri` to prevent future regressions
  - **Reported By**: turbomcpstudio dogfood team via RESOURCE_READ_ISSUE.md
  - **Severity**: Critical - Completely broke resource reading functionality

## [2.0.1] - 2025-10-19

### Fixed

- **Resource Listing Metadata Loss**: Fixed critical bug where `Client::list_resources()` was discarding resource metadata
  - **Issue**: Method was returning only URIs (`Vec<String>`), throwing away all metadata from server
  - **Impact**: Broke applications like turbomcpstudio that needed resource names, descriptions, MIME types
  - **Root Cause**: Implementation was mapping `ListResourcesResult::resources` to just URIs instead of returning full `Resource` objects
  - **Fix**: Changed return type from `Vec<String>` to `Vec<Resource>` per MCP 2025-06-18 spec
  - **Breaking Change**: No - `Resource` type was already exported and clients can access `.uri` field
  - **Files Changed**:
    - `turbomcp-client/src/client/operations/resources.rs` - Core fix to return full Resource objects
    - `turbomcp-cli/src/executor.rs` - Updated to handle Resource objects
    - `turbomcp-client/src/lib.rs` - Updated documentation examples
    - `turbomcp/examples/comprehensive.rs` - Enhanced to display resource metadata
    - `turbomcp/examples/unix_client.rs` - Updated to use resource.uri field
  - **Reported By**: turbomcpstudio dogfood team
  - **Severity**: High - Breaks core resource functionality

## [2.0.0] - 2025-10-18

### Added

- **Rich Tool Descriptions with Metadata**: Enhanced `#[tool]` macro now supports comprehensive metadata fields
  - **New fields**: `description`, `usage`, `performance`, `related`, `examples`
  - **LLM Optimization**: All fields combined into pipe-delimited description for better decision-making
  - **Backward Compatible**: Simple string syntax still supported
  - **Impact**: Improved LLM understanding of when/why/how to use tools
  - **Example**: New `rich_tool_descriptions.rs` example demonstrating all metadata fields
  - **Commit**: `aae59f8`

- **MCP STDIO Transport Compliance Enhancements**: Comprehensive specification compliance with validation
  - **Strict Validation**: Embedded newlines (LF/CR/CRLF) detection and rejection
  - **Compliance Documentation**: Detailed checklist in module documentation
  - **Test Coverage**: Comprehensive test suite for newline validation scenarios
  - **Spec Clarification**: Literal newline bytes forbidden, escaped `\n` in JSON strings allowed
  - **Error Messages**: MCP-specific compliance context in validation errors
  - **Impact**: Prevents message framing issues in production environments
  - **Commit**: `c2b4032`

### Fixed

- **Publish Script**: Minor fixes to release automation
  - **Commit**: `0b6e6a3`

### Improved

- **Examples Documentation**: Updated to reflect rich tool descriptions example
  - **Updated**: Example count from 17 to 18 examples
  - **Added**: rich_tool_descriptions.rs to quick start commands and examples table
  - **Commit**: `6e3b211`

## [2.0.0-rc.3] - 2025-10-18

### Removed

- **Progress Reporting System**: Removed experimental progress reporting infrastructure
  - **Rationale**: Progress reporting was not part of MCP 2025-06-18 spec and added unnecessary complexity
  - **Files removed**: All progress-related handler references and test utilities
  - **Impact**: Cleaner codebase focused on MCP compliance
  - **Commits**: `046cfe8`, `01bfc26`, `5ed2049`, `efa927b`, `d3559ce`

### Added

- **Enhanced Tool Attributes with Rich Metadata**: Macro system now supports comprehensive tool metadata
  - **New attributes**: Support for more granular tool definition and configuration
  - **Impact**: Better tooling and IDE support for MCP server development
  - **Commit**: `723fb20`

- **Comprehensive Schema Builder Functions for Elicitation API**: New builder functions for elicitation schemas
  - **Purpose**: Simplify and standardize elicitation form creation
  - **Impact**: More ergonomic API for server-initiated forms
  - **Commit**: `a57dac2`

- **Comprehensive Audit Reports and Analysis Tools**: Documentation tools for codebase analysis
  - **Purpose**: Enhanced visibility into codebase structure and metrics
  - **Impact**: Better development tooling and auditing capabilities
  - **Commit**: `7a41a03`

### Changed

- **Simplified Feature Flags for WebSocket Functionality**: WebSocket feature gates now cleaner
  - **Impact**: Reduced feature flag complexity and interdependencies
  - **Commit**: `a15edc1`

- **Simplified HTTP Feature Compilation Guards**: Removed redundant conditional compilation
  - **Impact**: Cleaner feature gate logic
  - **Commit**: `20e2692`

- **Improved DPOP Module Implementation**: Cleaned up DPOP crate structure
  - **Impact**: Better maintainability and clearer code organization
  - **Commit**: `c17d2d4`

- **Minor Cleanup in Core Modules and Examples**: General codebase polish
  - **Commit**: `69e3089`

### Improved

- **Build Automation**: Makefile and build scripts enhanced for better CI/CD integration
  - **Changes**: Improved automation workflow and test execution
  - **Commits**: `c81f20d`, `0633b64`

- **Test Suite Modernization**: Comprehensive test improvements
  - **Impact**: Better test coverage and modernized testing patterns
  - **Commit**: `c8d4f0c`

- **Security Builder and Testing**: Enhanced transport security implementation
  - **Commit**: `412570f`

- **Documentation and Examples**: Updated root README and examples for clarity
  - **Commits**: `31f82e7`, `d0773db`, `8024198`

### Quality

- **Added #[must_use] Attributes**: Compiler hints to prevent accidental discarding of important values
  - **Impact**: Better compiler feedback for common mistakes
  - **Commit**: `3dd833f`

## [2.0.0-rc.2] - 2025-10-16

### 🎯 **MAJOR FEATURES**

#### Architectural Unification - ALL Transports Now MCP Compliant
- **CRITICAL FIX**: Unified transport runtime implementations to eliminate duplication and protocol drift
  - ✅ **Single Source of Truth**: All transports (STDIO/TCP/Unix/HTTP/WebSocket) now use `turbomcp-server` runtime
  - ✅ **MCP 2025-06-18 Compliance**: Complete compliance across ALL transport types
  - ✅ **Zero Duplication**: Removed ~2,200 lines of duplicate code
  - **Impact**: Eliminated potential for implementation drift between macro and ServerBuilder patterns

#### HTTP & WebSocket Bidirectional Support via ServerBuilder
- ✅ **HTTP/SSE Bidirectional**: Full support for elicitation, sampling, roots, ping
- ✅ **WebSocket Bidirectional**: Complete bidirectional support matching macro pattern
- **Implementation**: Factory patterns with per-connection/per-session dispatchers
- **Result**: ✅ **ALL transports now fully MCP-compliant via ServerBuilder**

#### Critical Bug Fixes

**Sampling Request ID Correlation (CRITICAL)** - Breaking Change for 2.0
- **Problem**: Clients couldn't correlate sampling request rejections with server requests
  - Handler trait did NOT receive JSON-RPC `request_id`
  - Clients forced to generate their own UUIDs
  - User rejections sent with WRONG ID
- **Solution**: Added `request_id: String` parameter to handler traits
  - ✅ Client-side: `SamplingHandler::handle_create_message(request_id, request)`
  - ✅ Server-side: `SamplingHandler::handle(request_id, request)`
  - ✅ User rejections now complete immediately (< 100ms, not 60s)
- **Breaking Change**: All `SamplingHandler` implementations MUST add `request_id` parameter
- **Justification**: Pre-release critical bug fix enforcing MCP JSON-RPC 2.0 compliance

**WebSocket Deadlock (CRITICAL - P0)**
- **Problem**: Sampling/elicitation requests timed out after 60 seconds (response time: 60s)
- **Circular Deadlock**: receive_loop waits for handler → handler waits for response → response waits for receive_loop
- **Solution**: Spawn request handlers in separate tokio tasks to keep receive_loop non-blocking
- **Result**: Response time: 60s → 0ms (instant)

**HTTP Session ID Generation**
- **Problem**: Server was rejecting SSE connections without session ID (400 Bad Request)
- **Solution**: Server now generates session ID and sends to client (per MCP spec)
- **Impact**: HTTP/SSE sampling, elicitation, roots, ping operations now work correctly

### 🏗️ **ARCHITECTURAL CHANGES**

- **Removed Duplicate Runtimes** (~2,200 lines):
  - ❌ `crates/turbomcp/src/runtime/stdio_bidirectional.rs` (484 lines)
  - ❌ `crates/turbomcp/src/runtime/http_bidirectional.rs` (19KB)
  - ❌ `crates/turbomcp/src/runtime/websocket_server.rs` (726 lines)
  - ✅ **Replaced with**: Re-exports from canonical `turbomcp-server/src/runtime/`

- **Added Missing `Clone` Trait Bounds** to Handler Types
  - Enables concurrent handler execution in tokio tasks
  - Required for proper async spawning pattern

- **Unified ServerBuilder Pattern**:
  - Macro-generated code now uses `create_server()` → ServerBuilder → canonical runtime
  - Single implementation path for all transport types

### ✨ **NEW FEATURES**

- **Release Management Infrastructure**:
  - `scripts/check-versions.sh` - Validates version consistency (224 lines)
  - `scripts/update-versions.sh` - Safe version updates with confirmation (181 lines)
  - `scripts/publish.sh` - Dependency-ordered publishing (203 lines)
  - Enhanced `scripts/prepare-release.sh` - Improved validation workflow

- **Feature Combination Testing**:
  - `scripts/test-feature-combinations.sh` - Tests 10 critical feature combinations
  - Prevents feature gate leakage and compatibility issues

- **HTTP Transport Support**: Re-enabled HTTP client exports
  - Added `VERSION` and `CRATE_NAME` constants to turbomcp-client
  - Re-exported `StreamableHttpClientTransport`, `RetryPolicy`, `StreamableHttpClientConfig`

### 🔧 **IMPROVEMENTS**

- **Error Code Preservation**: Protocol errors now properly preserved through server layer
  - Error codes like `-1` (user rejection) maintained instead of converting to `-32603`
  - Added `ServerError::Protocol` variant
  - Proper error propagation: client → server → calling client

- **Error Messages**: JSON-RPC error codes now semantically correct in all scenarios
  - User rejection: `-1` (not `-32603`)
  - Not found: `-32004` (not `-32603`)
  - Authentication: `-32008` (not `-32603`)

- **Feature Compatibility**: Various Cargo.toml and module updates for better feature gate isolation
  - Updated feature dependencies across all crates
  - Improved runtime module feature handling
  - Better server capabilities and error handling with features

- **Documentation**: Enhanced across all crates
  - Added feature requirement docs to generated transport methods
  - Simplified main README with focused architecture section
  - Improved benchmark and demo documentation
  - Standardized crate-level documentation

- **Debug Implementation**: Added missing `Debug` derive to `WebSocketServerDispatcher`

### 📊 **BUILD STATUS**

- ✅ All 1,165 tests pass
- ✅ Zero regressions
- ✅ Full MCP 2025-06-18 compliance verified across all transports

## [2.0.0-rc.1] - 2025-10-11

### 🐛 **BUG FIXES**

#### TransportDispatcher Clone Implementation (Critical)
- **FIXED**: Manual `Clone` implementation for `TransportDispatcher<T>` removing unnecessary `T: Clone` bound
- **IMPACT**: TCP and Unix Socket transports now compile correctly
- **ROOT CAUSE**: `#[derive(Clone)]` incorrectly required `T: Clone` when only `Arc<T>` needed cloning
- **SOLUTION**: Manual implementation clones `Arc<T>` without requiring `T: Clone`
- **LOCATION**: `crates/turbomcp-server/src/runtime.rs:395-406`

#### SSE Test Conditional Compilation
- **FIXED**: SSE test functions now correctly handle `#[cfg(feature = "auth")]` conditional compilation
- **IMPACT**: Tests compile with and without `auth` feature enabled
- **LOCATION**: `crates/turbomcp/src/sse_server.rs:615,631,656`

#### TCP Client Example Error Handling
- **FIXED**: Address parsing in TCP client example using `.expect()` instead of `?`
- **IMPACT**: Example compiles without custom error type conversions
- **LOCATION**: `crates/turbomcp/examples/tcp_client.rs:28-29`

#### TCP/Unix Client Example Imports and Feature Gates
- **FIXED**: Import transport types directly from `turbomcp_transport`
- **FIXED**: Added `required-features` declarations for TCP/Unix examples
- **ROOT CAUSE**: Examples compiled without features, `turbomcp::prelude` guards exports with `#[cfg(feature)]`
- **SOLUTION 1**: Import directly from `turbomcp_transport` (always available)
- **SOLUTION 2**: Add `required-features` to skip examples when features disabled
- **IMPACT**: Examples only compile when features enabled, preventing feature mismatch errors
- **LOCATION**: `crates/turbomcp/examples/{tcp_client.rs:16-17, unix_client.rs:17-18}`, `Cargo.toml:157-172`

### 📚 **DOCUMENTATION IMPROVEMENTS**

#### Transport Protocol Clarification
- **UPDATED**: Main README to distinguish MCP standard transports from custom extensions
- **CLARIFIED**: STDIO and HTTP/SSE are MCP 2025-06-18 standard transports
- **CLARIFIED**: TCP, Unix Socket, and WebSocket are MCP-compliant custom extensions
- **UPDATED**: Transport README with protocol compliance section
- **UPDATED**: Architecture diagram showing transport categorization

### ✅ **QUALITY ASSURANCE**

**Build Verification**:
- ✅ All features build successfully (`--all-features`)
- ✅ TCP transport builds successfully (`--features tcp`)
- ✅ Unix Socket transport builds successfully (`--features unix`)
- ✅ All examples compile cleanly

**Test Results**:
- ✅ 153 tests passed, 0 failed
- ✅ Zero clippy warnings with `-D warnings`
- ✅ All code formatted correctly

**MCP Compliance**:
- ✅ Full MCP 2025-06-18 specification compliance verified
- ✅ All standard transports (stdio, HTTP/SSE) compliant
- ✅ Custom transports preserve JSON-RPC and lifecycle requirements

## [2.0.0-rc] - 2025-10-09

### 🌟 **RELEASE HIGHLIGHTS**

**TurboMCP 2.0.0 represents a complete architectural overhaul focused on clean minimal core + progressive enhancement.**

**Key Achievements**:
- ✅ **Progressive Enhancement**: Minimal by default (stdio only), opt-in features for advanced needs
- ✅ **Zero Technical Debt**: No warnings, no TODOs, no FIXMEs
- ✅ **Security**: 1 mitigated vulnerability, 1 compile-time warning only
- ✅ **Clean Architecture**: RBAC removed (application-layer concern)
- ✅ **Latest Toolchain**: Rust 1.90.0 + 62 dependency updates
- ✅ **Production Ready**: All examples compile, all tests pass, strict clippy compliance

### 🎯 **BREAKING CHANGES**

#### RBAC Removal - Architectural Improvement
- **REMOVED**: RBAC/authorization feature from protocol layer
- **RATIONALE**: Authorization is an application-layer concern, not protocol-layer
- **IMPACT**: Cleaner separation of concerns, follows industry best practices
- **MIGRATION**: Implement authorization in your application layer (see `RBAC-REMOVAL-SUMMARY.md`)
- **BENEFIT**: Eliminated `casbin` dependency and `instant` unmaintained warning
- **SECURITY**: Reduced attack surface, removed unmaintained runtime dependency

#### SharedClient Removal - Architectural Improvement
- **REMOVED**: `SharedClient` wrapper (superseded by directly cloneable `Client<T>`)
- **RATIONALE**: `Client<T>` is now Arc-wrapped internally, making SharedClient redundant
- **IMPACT**: Simpler API with no wrapper needed for concurrent access
- **MIGRATION**: Replace `SharedClient::new(client)` with direct `client.clone()`
- **BENEFIT**: Reduced API surface, cleaner concurrent patterns following Axum/Tower standard
- **NOTE**: `SharedTransport` remains available for sharing transports across multiple clients

#### Default Feature Changes
- **BREAKING**: Default features changed to `["stdio"]` (minimal by default)
- **RATIONALE**: Progressive enhancement - users opt-in to features they need
- **MIGRATION**: Enable features explicitly: `turbomcp = { version = "2.0.0-rc", features = ["full"] }`

### 🏗️ **MAJOR REFACTORING: Clean Minimal Core**

#### New Crate Architecture (10 Total Crates)
- **NEW**: `turbomcp-auth` - OAuth 2.1 authentication (optional, 1,824 LOC)
- **NEW**: `turbomcp-dpop` - DPoP RFC 9449 implementation (optional, 7,160 LOC)
- **MODULAR**: Independent crates for protocol, transport, server, and client
- **PROGRESSIVE**: Features are opt-in via feature flags
- **CORE**: Context module decomposed from monolithic 2,046-line file into 8 focused modules:
  - `capabilities.rs` - Capability trait definitions
  - `client.rs` - Client session and identification
  - `completion.rs` - Completion context handling
  - `elicitation.rs` - Interactive form handling
  - `ping.rs` - Health check contexts
  - `request.rs` - Core request/response context
  - `server_initiated.rs` - Server-initiated communication
  - `templates.rs` - Resource template contexts
- **PROTOCOL**: Types module decomposed from monolithic 2,888-line file into 12 focused modules:
  - Individual modules for capabilities, completion, content, core, domain, elicitation, initialization, logging, ping, prompts, requests, resources, roots, sampling, and tools
- **IMPROVED**: Enhanced code maintainability with zero breaking changes to public API

### ⚡ **PERFORMANCE OPTIMIZATIONS**
- **ENHANCED**: Zero-copy message processing with extensive `bytes::Bytes` integration
- **NEW**: Advanced `ZeroCopyMessage` type for ultra-high throughput scenarios
- **OPTIMIZED**: Message processing with lazy deserialization and minimal allocations
- **IMPROVED**: SIMD-accelerated JSON processing with `sonic-rs` and `simd-json`

### 🔐 **SECURITY ENHANCEMENTS**
- **REMOVED**: RBAC feature eliminated `instant` unmaintained dependency (RUSTSEC-2024-0384)
- **IMPROVED**: Dependency cleanup with 13 fewer dependencies (-2.2%)
- **AUDIT**: Only 1 known vulnerability (RSA timing - mitigated by P-256 recommendation)
- **AUDIT**: Only 1 unmaintained warning (paste - compile-time only, zero runtime risk)
- **NEW**: Security validation module in `turbomcp-core` with path security utilities
- **ADDED**: `validate_path()`, `validate_path_within()`, `validate_file_extension()` functions
- **INTEGRATED**: Security features from dissolved security crate into core framework
- **DOCUMENTED**: P-256 recommended as default DPoP algorithm (not affected by RSA timing attack)

### 🛠️ **API IMPROVEMENTS**
- **IMPROVED**: Enhanced registry system with handler statistics and analytics
- **ADDED**: `EnhancedRegistry` with performance tracking
- **ENHANCED**: Session management with improved analytics and cleanup
- **REFINED**: Error handling with comprehensive context preservation


### 🔧 **INTERNAL IMPROVEMENTS**
- **CLEANED**: Removed obsolete tests and legacy code
- **ENHANCED**: Test suite with comprehensive coverage of new modules
- **IMPROVED**: Build system and CI/CD pipeline optimizations
- **MAINTAINED**: Zero clippy warnings and consistent formatting

### 🔨 **TOOLCHAIN & DEPENDENCY UPDATES**
- **UPDATED**: Rust toolchain from 1.89.0 → 1.90.0
- **UPDATED**: 62 dependencies to latest compatible versions:
  - `axum`: 0.8.4 → 0.8.6
  - `tokio-tungstenite`: 0.26.2 → 0.28.0
  - `redis`: 0.32.5 → 0.32.7
  - `serde`: 1.0.226 → 1.0.228
  - `thiserror`: 2.0.16 → 2.0.17
  - And 57 more transitive updates
- **ADDED**: `futures` dependency to `turbomcp-dpop` (previously missing)

### 🐛 **BUG FIXES & CODE QUALITY**
- **FIXED**: Documentation warning in `zero_copy.rs` (added missing doc comment)
- **FIXED**: Feature gate naming consistency (`dpop-redis` → `redis-storage`, `dpop-test-utils` → `test-utils`)
- **FIXED**: Removed unused middleware import in `turbomcp/router.rs`
- **FIXED**: Removed unused `McpResult` import in `turbomcp/transport.rs`
- **FIXED**: Removed unused `RateLimitConfig` import in `turbomcp-server/core.rs`
- **FIXED**: Clippy warnings (empty line after doc comments, manual is_multiple_of)
- **RESULT**: Zero compiler warnings, zero clippy warnings with `-D warnings`

### 🛡️ **BACKWARD COMPATIBILITY**
- **BREAKING**: RBAC feature removed (see migration notes below)
- **BREAKING**: Default features changed to minimal (`["stdio"]`)
- **COMPATIBLE**: Existing auth, rate-limiting, validation features unchanged
- **PROTOCOL**: Maintains complete MCP 2024-11-05 specification compliance

### 📦 **MIGRATION NOTES**

#### RBAC Removal (Breaking Change)
If you were using the RBAC feature:
```toml
# OLD (no longer works)
turbomcp-server = { version = "2.0.0-rc", features = ["rbac"] }

# NEW (implement in your application)
# See RBAC-REMOVAL-SUMMARY.md for migration patterns
```
- **Why**: Authorization is application-layer concern, not protocol-layer
- **How**: Implement RBAC in your application using JWT claims or external policy engine
- **Examples**: See `RBAC-REMOVAL-SUMMARY.md` for complete migration guide

#### Default Features
```toml
# OLD (1.x - everything enabled)
turbomcp = "1.x"  # Had all features by default

# NEW (2.0 - minimal by default)
turbomcp = { version = "2.0.0-rc", features = ["full"] }  # Opt-in to features
```

#### Crate Consolidation
- `turbomcp_dpop::*` → `turbomcp::auth::dpop::*`
- Security utilities now in `turbomcp_core::security`

#### Feature Gate Names
- `dpop-redis` → `redis-storage`
- `dpop-test-utils` → `test-utils`

See `MIGRATION.md` for complete upgrade guide.

### 📊 **METRICS & QUALITY**

**Codebase Quality**:
- ✅ Compiler warnings: **0**
- ✅ Clippy warnings (with `-D warnings`): **0**
- ✅ Technical debt markers (TODO/FIXME): **0**
- ✅ All examples compile: **Yes**
- ✅ All tests pass: **Yes**

**Security Posture**:
- 🔒 Known vulnerabilities: **1 (mitigated)**
  - RSA timing sidechannel: Use P-256 instead (recommended in docs)
- ⚠️ Unmaintained dependencies: **1 (informational only)**
  - paste v1.0.15: Compile-time proc macro only, zero runtime risk, HSM feature only
- ✅ Security improvements: Removed `instant` unmaintained runtime dependency

**Dependency Management**:
- 📦 Feature-gated dependencies: Pay only for what you use
- 📉 Cleanup: **-13 dependencies** (-2.2% from 1.x)

**Release Status**: 🟢 **PRODUCTION READY**

## [1.1.0] - 2025-09-24

### 🔐 **NEW MAJOR FEATURE: RFC 9449 DPoP Security Suite**
- **ADDED**: Complete RFC 9449 Demonstration of Proof-of-Possession (DPoP) implementation
- **NEW**: `turbomcp-dpop` crate with OAuth 2.0 security enhancements
- **SECURITY**: Cryptographic binding of access tokens to client keys preventing token theft
- **ENTERPRISE**: Multi-store support (Memory, Redis, HSM) for different security requirements
- **ALGORITHMS**: ES256, RS256 support with automatic key rotation policies
- **HSM**: YubiHSM2 and PKCS#11 integration for enhanced security

### 🏗️ **NEW MAJOR FEATURE: Type-State Capability Builders**
- **REVOLUTIONARY**: Const-generic type-state builders with compile-time validation
- **SAFETY**: Impossible capability configurations are unrepresentable in type system
- **PERFORMANCE**: Zero-cost abstractions - all validation at compile time
- **DEVELOPER EXPERIENCE**: Compile-time errors prevent runtime capability misconfigurations
- **TURBOMCP EXCLUSIVE**: Advanced features like SIMD optimization hints and enterprise security
- **CONVENIENCE**: Pre-configured builders for common patterns (full-featured, minimal, sampling-focused)

### ⚡ **PERFORMANCE & QUALITY IMPROVEMENTS**
- **MODERNIZED**: All benchmarks updated to use `std::hint::black_box` (eliminated deprecation warnings)
- **ENHANCED**: Redis AsyncIter with `safe_iterators` feature for safer iteration
- **IMPROVED**: WebSocket transport compatibility with tokio-tungstenite v0.27.0
- **OPTIMIZED**: Message::Text API usage for improved performance
- **FIXED**: All doctest compilation errors and import issues

### 📊 **DEPENDENCY & SECURITY UPDATES**
- **UPDATED**: All workspace dependencies to latest stable versions
- **SECURITY**: Eliminated all deprecated API usage across the codebase
- **COMPATIBILITY**: Enhanced WebSocket examples with real-time bidirectional communication
- **QUALITY**: Comprehensive test suite improvements and validation

### 🛡️ **BACKWARD COMPATIBILITY**
- **GUARANTEED**: 100% backward compatibility with all v1.0.x applications
- **ZERO BREAKING CHANGES**: All existing code continues to work unchanged
- **MIGRATION**: Optional upgrade path to new type-safe builders
- **PROTOCOL**: Maintains complete MCP 2025-06-18 specification compliance

### 📚 **DOCUMENTATION & EXAMPLES**
- **NEW**: Comprehensive DPoP integration guide with production examples
- **NEW**: Interactive type-state builder demonstration (`examples/type_state_builders_demo.rs`)
- **ENHANCED**: API documentation with advanced usage patterns
- **IMPROVED**: WebSocket transport examples with real-world patterns

## [1.0.13] - Never released

### 🔒 **SECURITY HARDENING - ZERO VULNERABILITIES ACHIEVED**
- **ELIMINATED**: RSA Marvin Attack vulnerability (`RUSTSEC-2023-0071`) through strategic `sqlx` removal
- **ELIMINATED**: Unmaintained `paste` crate vulnerability (`RUSTSEC-2024-0436`) via `rmp-serde` → `msgpacker` migration
- **IMPLEMENTED**: Comprehensive `cargo-deny` security policy with MIT-compatible license restrictions
- **OPTIMIZED**: Dependency security surface with strategic removal of vulnerable dependency trees

### ⚡ **COMPREHENSIVE BENCHMARKING INFRASTRUCTURE**
- **NEW**: Enterprise-grade criterion benchmarking with automated regression detection (5% threshold)
- **NEW**: Cross-platform performance validation (Ubuntu, Windows, macOS) with GitHub Actions integration
- **NEW**: Historical performance tracking with git commit correlation and baseline management
- **ACHIEVED**: Performance targets - <1ms tool execution, >100k messages/sec, <1KB overhead per request
- **ADDED**: Comprehensive benchmark coverage across all critical paths (core, framework, end-to-end)

### 🚀 **ENHANCED CLIENT LIBRARY**
- **ENHANCED**: Advanced LLM backend support with production-grade Anthropic and OpenAI implementations
- **NEW**: Interactive elicitation client with real-time user input capabilities
- **IMPROVED**: Comprehensive conversation context management and error handling
- **OPTIMIZED**: HTTP client configuration with proper timeouts and user agent versioning

### 🏗️ **CORE INFRASTRUCTURE IMPROVEMENTS**
- **ENHANCED**: MessagePack serialization with `msgpacker` integration (temporary test workaround in place)
- **IMPROVED**: Macro system with better compile-time routing and automatic discovery
- **OPTIMIZED**: Message processing with enhanced format detection and validation

### 📊 **QUALITY ASSURANCE**
- **FIXED**: Test suite timeout issues through optimized compilation and execution
- **ENHANCED**: Comprehensive message testing with edge cases and boundary validation
- **IMPROVED**: Error handling and debugging capabilities across all crates
- **SYNCHRONIZED**: All crate versions to 1.0.13 with updated documentation

### 🛠️ **DEVELOPER EXPERIENCE**
- **NEW**: `scripts/run_benchmarks.sh` automation with multiple execution modes
- **ENHANCED**: Documentation with comprehensive benchmarking guide and production examples
- **IMPROVED**: Build system performance and caching optimizations
- **ADDED**: Performance monitoring and regression detection in CI/CD pipeline

## [1.0.10] - 2025-09-21

### 🚨 **CRITICAL MCP 2025-06-18 COMPLIANCE FIX**
- **SharedClient Protocol Compliance**: Fixed critical gap where SharedClient was missing key MCP protocol methods
  - ✅ **Added `complete()`**: Argument completion support (completion/complete) for IDE-like experiences
  - ✅ **Added `list_roots()`**: Filesystem roots listing (roots/list) for boundary understanding
  - ✅ **Added elicitation handlers**: Server-initiated user information requests (elicitation/create)
  - ✅ **Added bidirectional handlers**: Log and resource update handler registration
  - ✅ **Added handler query methods**: `has_*_handler()` methods for capability checking
- **Full MCP 2025-06-18 Compliance**: SharedClient now provides complete protocol compliance matching regular Client
- **Zero Breaking Changes**: All additions are purely additive maintaining full backward compatibility
- **Enhanced Documentation**: Updated README to reflect complete protocol support and capabilities

### 🔧 **Quality Improvements**
- **Perfect Thread Safety**: All new SharedClient methods maintain zero-overhead Arc/Mutex abstractions
- **Consistent API Surface**: All methods use identical signatures to regular Client for drop-in replacement
- **Complete Doctest Coverage**: All new methods include comprehensive examples and usage patterns
- **Type Safety**: Maintains compile-time guarantees and proper error handling throughout

### 📋 **Post-Release Audit Results**
This release addresses compliance gaps identified during comprehensive MCP 2025-06-18 specification audit:
- ✅ **Specification Compliance**: 100% compliant with MCP 2025-06-18 including latest elicitation features
- ✅ **Transport Support**: All 5 transport protocols support complete MCP feature set
- ✅ **Server Implementation**: Full server-side MCP method coverage verified
- ✅ **Test Coverage**: All new functionality tested with comprehensive test suite

## [1.0.9] - 2025-09-21

### 🔄 Shared Wrapper System (MAJOR FEATURE)
- **Thread-Safe Concurrency Abstractions**: Complete shared wrapper system addressing Arc/Mutex complexity feedback
  - ✅ **SharedClient**: Thread-safe client wrapper enabling concurrent MCP operations
  - ✅ **SharedTransport**: Multi-client transport sharing with automatic connection management
  - ✅ **SharedServer**: Server wrapper with safe consumption pattern for management scenarios
  - ✅ **Generic Shareable Pattern**: Reusable trait-based abstraction for all shared wrappers
- **Zero Overhead Abstractions**:
  - ✅ **Same Performance**: Identical runtime performance to direct Arc/Mutex usage
  - ✅ **Hidden Complexity**: Encapsulates synchronization primitives behind ergonomic APIs
  - ✅ **MCP Protocol Compliant**: Maintains all MCP semantics in shared contexts
  - ✅ **Drop-in Replacement**: Works with existing code without breaking changes
- **Production-Ready Patterns**:
  - ✅ **Consumption Safety**: ConsumableShared<T> prevents multiple consumption of server-like objects
  - ✅ **Library Integration**: Seamless integration with external libraries requiring Arc<Mutex<Client>>
  - ✅ **Concurrent Access**: Multiple tasks can safely access clients and transports simultaneously
  - ✅ **Resource Management**: Proper cleanup and lifecycle management in multi-threaded scenarios

### 🚀 Enhanced Concurrency Support
- **Concurrent Operation Examples**:
  - Multiple threads calling tools simultaneously through SharedClient
  - Transport sharing between multiple client instances
  - Management dashboard integration with SharedServer consumption
  - Complex multi-client architectures with single transport
- **Developer Experience Improvements**:
  - ✅ **Ergonomic APIs**: Simple `.clone()` operations instead of complex Arc/Mutex patterns
  - ✅ **Type Safety**: Compile-time guarantees preventing common concurrency mistakes
  - ✅ **Clear Documentation**: Comprehensive examples and usage patterns in all crate READMEs
  - ✅ **Seamless Migration**: Existing code continues working; shared wrappers are additive

### 📚 Documentation Excellence
- **Comprehensive Documentation Updates**:
  - ✅ **All Crate READMEs Updated**: SharedClient, SharedTransport, SharedServer sections added
  - ✅ **Usage Examples**: Detailed examples showing concurrent patterns and integration
  - ✅ **Architecture Guidance**: Clear guidance on when and how to use shared wrappers
  - ✅ **Build Status Fix**: Consistent GitHub Actions badge format across all READMEs
- **Generic Pattern Documentation**:
  - ✅ **Shareable Trait**: Complete documentation of the reusable abstraction pattern
  - ✅ **Implementation Examples**: Both Shared<T> and ConsumableShared<T> patterns documented
  - ✅ **Best Practices**: Guidelines for implementing custom shared wrappers

### 🔧 Quality & Maintenance
- **Version Consistency**: Updated all crate versions to 1.0.9 with proper internal dependency alignment
- **Code Quality**: Maintained zero clippy warnings and perfect formatting standards
- **Test Coverage**: All unit tests (392 tests) passing across all crates
- **Build System**: Consistent build status reporting across all documentation

## [1.0.8] - 2025-09-21

### 🔐 OAuth 2.1 MCP Compliance (MAJOR FEATURE)
- **Complete OAuth 2.1 Implementation**:
  - ✅ **RFC 8707 Resource Indicators**: MCP resource URI binding for token scoping
  - ✅ **RFC 9728 Protected Resource Metadata**: Discovery and validation endpoints
  - ✅ **RFC 7591 Dynamic Client Registration**: Runtime client configuration
  - ✅ **PKCE Support**: Enhanced security with Proof Key for Code Exchange
  - ✅ **Multi-Provider Support**: Google, GitHub, Microsoft OAuth 2.0 integration
- **Security Hardening**:
  - ✅ **Redirect URI Validation**: Prevents open redirect attacks
  - ✅ **Domain Whitelisting**: Environment-based host validation
  - ✅ **Attack Vector Prevention**: Protection against injection and traversal attacks
  - ✅ **Production Security**: Comprehensive security level configuration
- **MCP-Specific Features**:
  - ✅ **Resource Registry**: MCP resource metadata with RFC 9728 compliance
  - ✅ **Bearer Token Methods**: Multiple authentication methods support
  - ✅ **Auto Resource Indicators**: Automatic MCP resource URI detection
  - ✅ **Security Levels**: Standard, Enhanced, Maximum security configurations

### 🚀 MCP STDIO Protocol Compliance
- **Logging Compliance**: Fixed demo application to output ONLY JSON-RPC messages
  - ✅ **Zero Stdout Pollution**: No logging, banners, or debug output on stdout
  - ✅ **Pure Protocol Communication**: MCP STDIO transport compliant
  - ✅ **Clean Demo Application**: Production-ready MCP server demonstration

### 🧹 Code Quality & Maintenance (MAJOR CLEANUP)
- **Zero-Tolerance Quality Standards Achieved**:
  - ✅ **100% Clippy Clean**: Fixed all clippy warnings with `-D warnings` across entire workspace
  - ✅ **Perfect Formatting**: All code consistently formatted with `cargo fmt`
  - ✅ **All Tests Passing**: Complete test suite (800+ tests) running without issues
  - ✅ **Modern Rust Patterns**: Converted all nested if statements to use let chains
  - ✅ **Memory Management**: Removed unnecessary explicit `drop()` calls for better clarity

### 🗂️ Project Cleanup & Organization
- **Removed Vestigial Files**:
  - Cleaned up 7 `.disabled` example files that were no longer needed
  - Removed: `transport_*_client.rs.disabled` and `transport_*_server.rs.disabled` files
  - Eliminated legacy code artifacts from development phase
- **Documentation Overhaul**:
  - **Updated Examples README**: Complete rewrite with accurate current example inventory
  - **35 Production-Ready Examples**: All examples documented and categorized properly
  - **Clear Learning Path**: Progression from beginner to advanced with numbered tutorials
  - **Transport Coverage**: Complete coverage of all 5 transport types (STDIO, TCP, HTTP/SSE, WebSocket, Unix)

### 🛠️ Technical Improvements
- **Collapsible If Statement Fixes**: 8+ instances converted to modern let chains pattern
  - `websocket_client.rs`: 2 collapsible if statements fixed
  - `transport_websocket_client.rs`: 6 collapsible if statements fixed
  - `unix_socket_client.rs`: 1 collapsible if statement fixed
- **Drop Non-Drop Warnings**: Fixed unnecessary explicit drops in test files
  - `real_end_to_end_working_examples.rs`: Removed 2 explicit drop calls for tokio WriteHalf types
- **Unix Transport Test Fixes**: Updated test expectations to match actual implementation
  - Fixed capabilities test to expect 1MB (not 64MB) message size limit
  - Updated error message expectations for disconnected transport scenarios

### 📚 Documentation Standards
- **Example Categories**: Clear organization by transport type, complexity, and use case
- **Quality Guarantees**: All examples follow high standards
- **Learning Progression**: 11 numbered tutorial examples from basic to advanced
- **Transport Comparison**: Legacy vs. current transport example organization
- **35 Total Examples**: Complete inventory with proper categorization

### 🔧 Development Experience
- **Make Test Integration**: Full compatibility with project's `make test` command
- **CI/CD Ready**: All quality checks pass automated testing pipeline
- **Zero Technical Debt**: Eliminated all placeholder code and TODOs from examples
- **Consistent Standards**: Unified code style and documentation across all examples

### 🏆 Quality Metrics Achieved
- **Clippy**: Zero warnings with strict `-D warnings` enforcement
- **Formatting**: 100% consistent code formatting across 35 examples
- **Tests**: All integration and unit tests passing
- **Documentation**: Complete and accurate example documentation
- **Examples**: 35 fully-functional examples

## [1.0.6] - 2025-09-10

### 🔌 Enterprise Plugin System (NEW)
- **Complete Plugin Architecture**: Production-ready middleware system for Client
  - `ClientPlugin` trait for custom plugin development
  - `PluginRegistry` for managing plugin lifecycle
  - `RequestContext` and `ResponseContext` for plugin state
  - Before/after request hooks for all 13 MCP protocol methods
- **Built-in Enterprise Plugins**:
  - **RetryPlugin**: Automatic retry with exponential backoff
  - **CachePlugin**: TTL-based response caching for performance
  - **MetricsPlugin**: Request/response metrics collection
- **Plugin Features**:
  - Zero-overhead when not in use
  - Transparent operation - no code changes needed
  - Composable - stack multiple plugins
  - Async-first design throughout
- **ClientBuilder Enhancement**: Fluent API for plugin registration
  ```rust
  ClientBuilder::new()
      .with_plugin(Arc::new(RetryPlugin::new(config)))
      .with_plugin(Arc::new(CachePlugin::new(config)))
      .build(transport)
  ```

### 🛠️ API Improvements
- **Plugin Management Methods** on Client:
  - `register_plugin()` - Add plugins at runtime
  - `has_plugin()` - Check if plugin is registered
  - `get_plugin()` - Access specific plugin instance
  - `initialize_plugins()` - Initialize all plugins
  - `shutdown_plugins()` - Clean shutdown of plugins
- **Execute with Plugins**: Internal helper for middleware execution
  - Automatic plugin pipeline for all protocol calls
  - Request/response modification support
  - Error propagation through middleware chain

### 📚 Documentation & Examples
- **New Plugin Examples**:
  - Complete plugin implementation examples in `plugins/examples.rs`
  - Shows retry logic, caching, and metrics collection
  - Demonstrates custom plugin development

### 🔧 Technical Improvements
- **Zero-Tolerance Production Standards**: 
  - Removed all TODO comments from plugin system
  - Complete implementation of all plugin features
  - No placeholders or incomplete code
- **Error Handling**: Better error messages for plugin failures
- **Performance**: Plugin system adds <2% overhead when active

### 🐛 Bug Fixes
- Fixed clippy warnings about unnecessary borrows
- Fixed formatting inconsistencies in plugin code
- Updated all test assertions for new version

## [1.0.5] - 2025-09-09

### 🎯 Major Examples Overhaul
- **Reduced from 41 to 12 focused examples** (70% reduction)
- Created clear learning progression from basics to production
- Added comprehensive EXAMPLES_GUIDE.md with learning path
- New `06_architecture_patterns.rs` showing builder vs macro equivalence
- New `06b_architecture_client.rs` separate client for testing both patterns
- Consolidated all transport demos into `07_transport_showcase.rs`
- Merged all elicitation patterns into `08_elicitation_complete.rs`
- Fixed all compilation errors across examples
- Every example now works end-to-end without placeholders
- **New two-terminal HTTP examples**: `08_elicitation_server.rs` and `08_elicitation_client.rs` for real-world testing

### 🚀 Developer Experience Improvements
- **📢 Deprecation: Simplified Feature System** - `internal-deps` feature flag is now deprecated (will be removed in 2.0.0)
  - Core framework dependencies are now included automatically - no manual setup required!
  - **Migration**: Remove `internal-deps` from your feature lists for cleaner configuration
  - **Before**: `features = ["internal-deps", "stdio"]` → **After**: `features = ["minimal"]` or `features = ["stdio"]`
  - **Backwards compatible**: Old feature combinations still work but show deprecation warnings
  - **Rationale**: Eliminates user confusion since these dependencies were always required
- **Enhanced Error Handling**: New `McpErrorExt` trait with ergonomic error conversion methods
  - `.tool_error("context")?` instead of verbose `.map_err()` calls
  - `.network_error()`, `.protocol_error()`, `.resource_error()`, `.transport_error()` methods
  - Automatic `From` trait implementations for common error types (`std::io::Error`, `reqwest::Error`, `chrono::ParseError`, etc.)
- **Improved Prelude**: Enhanced documentation showing that `use turbomcp::prelude::*;` eliminates complex import chains
- **Better Feature Discovery**: Comprehensive 🎯 Feature Selection Guide in documentation and Cargo.toml
  - Clear recommendations for `minimal` vs `full` feature sets
  - Beginner-friendly guidance with specific use cases
  - Prominent placement of minimal features for basic tool servers
- **Comprehensive Method Documentation**: New 📚 Generated Methods Reference documenting all `#[server]` macro-generated methods
  - Transport methods (`run_stdio()`, `run_http()`, `run_tcp()`, etc.)
  - Metadata and testing methods (`server_info()`, tool metadata functions)
  - Context injection behavior and flexible parameter positioning

### ✨ New Features

#### 🎯 Complete MCP Protocol Support with New Attribute Macros
**MAJOR: Four new attribute macros completing MCP protocol coverage**

- **`#[completion]`** - Autocompletion handlers for intelligent parameter suggestions
  ```rust
  #[completion("Complete file paths")]
  async fn complete_path(&self, partial: String) -> McpResult<Vec<String>> {
      Ok(vec!["config.json".to_string(), "data.txt".to_string()])
  }
  ```
- **`#[elicitation]`** - Structured input collection from clients with schema validation
  ```rust
  #[elicitation("Collect user preferences")]
  async fn get_preferences(&self, schema: serde_json::Value) -> McpResult<serde_json::Value> {
      Ok(serde_json::json!({"theme": "dark", "language": "en"}))
  }
  ```
- **`#[ping]`** - Bidirectional health checks and connection monitoring
  ```rust
  #[ping("Health check")]
  async fn health_check(&self) -> McpResult<String> {
      Ok("Server is healthy".to_string())
  }
  ```
- **`#[template]`** - Resource template handlers with RFC 6570 URI templates
  ```rust
  #[template("users/{user_id}/profile")]
  async fn get_user_profile(&self, user_id: String) -> McpResult<String> {
      Ok(format!("Profile for user: {}", user_id))
  }
  ```

#### 🚀 Enhanced Client SDK with Completion Support
**NEW: `complete()` method in turbomcp-client**
```rust
let completions = client.complete("complete_path", "/usr/b").await?;
println!("Suggestions: {:?}", completions.values);
```

#### 🌐 Advanced Transport & Integration Features
- **Configurable HTTP Routes**: Enhanced `/mcp` endpoint with `run_http_with_path()` for custom paths
  - Default `/mcp` route maintained for compatibility
  - Flexible routing with `into_router_with_path()` for Axum integration
  - Support for existing router state preservation
- **Advanced Axum Integration**: Production-grade integration layer for existing Axum applications
  - State-preserving merge capabilities for "bring your own server" philosophy
  - Zero-conflict route merging with existing stateful routers
  - Tower service foundation for observability and error handling
- **Streamable HTTP Transport**: MCP 2025-06-18 compliant HTTP/SSE transport with streaming capabilities
- **Client Plugin System**: Extensible plugin architecture for client customization  
- **LLM Integration**: Comprehensive LLM provider system with sampling protocol
- **Bidirectional Handlers**: Full support for MCP handler types:
  - ElicitationHandler for server-initiated prompts
  - LogHandler for structured logging
  - ResourceUpdateHandler for file change notifications
- **Enhanced Builder API**: Improved ServerBuilder and ClientBuilder patterns

### 🛠 Improvements
- **Simplified API surface** while maintaining full functionality
- **Enhanced Cargo.toml**: Reorganized feature flags with clear descriptions and recommendations
- **Better error messages** and compile-time validation
- **Improved test coverage** with real integration tests (800+ tests passing)
- **Updated all dependencies** to latest versions
- **Enhanced documentation** with clear examples and comprehensive method reference
- **Ergonomic imports**: Single prelude import provides everything needed for most use cases
- **Production-ready error handling**: Comprehensive error conversion utilities eliminate boilerplate

### 🐛 Bug Fixes
- Fixed schema generation in macro system
- Resolved handler registration issues
- Fixed transport lifecycle management
- Corrected async trait implementations

### 📚 Documentation
- Complete examples guide with difficulty ratings
- Learning path from "Hello World" to production
- Feature matrix showing which examples demonstrate what
- Clear explanation of builder vs macro trade-offs

### 🏗 Internal Changes
- Cleaned up legacy code and unused files
- Improved module organization
- Better separation of concerns
- Consistent error handling patterns

## [1.0.4] - 2025-01-07

### Added
- Initial production release
- Core MCP protocol implementation
- Macro-based server definition
- Multi-transport support (STDIO, HTTP, WebSocket, TCP)
- Comprehensive tool and resource management
- Elicitation support for server-initiated prompts

## [1.0.3] - 2025-01-06

### Added
- Sampling protocol support
- Roots configuration
- Enhanced security features

## [1.0.2] - 2025-01-05

### Added
- OAuth 2.0 authentication
- Rate limiting
- CORS support

## [1.0.1] - 2025-01-04

### Added
- Basic MCP server functionality
- Tool registration system
- Resource management

## [1.0.0] - 2025-01-03

### Added
- Initial release
- Basic MCP protocol support
- STDIO transport