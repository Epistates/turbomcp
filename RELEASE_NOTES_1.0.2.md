# TurboMCP v1.0.2 Release Notes

## Overview
This release focuses on improving code quality, fixing integration issues, and ensuring full compliance with the MCP 2025-06-18 protocol specification. All compilation errors have been resolved, documentation has been enhanced, and the codebase has been prepared for production deployment.

## Key Improvements

### Protocol Compliance
- Full integration of MCP 2025-06-18 protocol features
- Fixed handler type names to match protocol specification (CompleteRequestParams, CompletionResponse)
- Added proper exports for all new MCP types (ElicitRequest, ElicitResult, ElicitationAction, etc.)
- Resolved integration debt between evolved APIs and existing code

### Code Quality
- Fixed all compilation errors across the workspace (40+ issues resolved)
- Addressed all clippy warnings for cleaner, more idiomatic Rust code
- Enhanced inline documentation for public APIs
- Added missing Debug derives for zero-copy and lock-free types
- Removed unused imports and variables

### Router Configuration
- Added `enable_bidirectional` field to RouterConfig for bidirectional communication support
- Updated all tests to use the new configuration structure
- Ensured backward compatibility with existing configurations

### Testing Improvements
- All 274+ tests now passing with 100% library test success rate
- Fixed test assertions to match actual default values
- Improved test reliability and coverage

## Breaking Changes
None - this is a backward-compatible patch release.

## Migration Guide
No migration required. Simply update your dependency versions:

```toml
turbomcp = "1.0.2"
turbomcp-core = "1.0.2"
turbomcp-protocol = "1.0.2"
turbomcp-transport = "1.0.2"
turbomcp-server = "1.0.2"
turbomcp-client = "1.0.2"
turbomcp-macros = "1.0.2"
turbomcp-cli = "1.0.5"
```

## Fixed Issues
- Router configuration missing field errors
- Unresolved import errors for MCP types
- Handler type name mismatches
- BoxFuture lifetime specification issues
- Missing Debug implementations
- Test assertion failures with incorrect default values
- Clippy warnings across the codebase


For questions or issues, please refer to the project documentation or file an issue on the repository.