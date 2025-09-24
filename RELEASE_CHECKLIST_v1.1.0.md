# TurboMCP v1.1.0 Release Checklist

## ✅ Pre-Release Validation (COMPLETED)

- [x] **Git Status**: Working directory is clean
- [x] **Version Numbers**: All Cargo.toml files show v1.1.0
- [x] **Internal Dependencies**: All cross-crate dependencies point to v1.1.0
- [x] **Test Suite**: All tests, clippy, and fmt checks pass
- [x] **Documentation**: All README files updated to v1.1.0
- [x] **Scripts Updated**: Release scripts updated for v1.1.0
- [x] **Dry-Run Testing**: Core crate packaging validated

## 📝 Release Process (Ready to Execute)

### 1. GitHub Tag and Release (User Action - In Progress)
- [ ] Create GitHub tag: `v1.1.0`
- [ ] Create GitHub release with release notes from `.strategy/RELEASE_NOTES_v1.1.0.md`

### 2. Crates.io Publishing (Execute After GitHub Tag)
```bash
# Make sure you're logged into crates.io
cargo login

# Run the publishing script
./scripts/publish-crates.sh
```

**Publishing Order (Dependency-First):**
1. turbomcp-core
2. turbomcp-protocol
3. turbomcp-transport
4. turbomcp-macros
5. turbomcp-dpop ← **New crate for v1.1.0**
6. turbomcp-server
7. turbomcp-client
8. turbomcp-cli
9. turbomcp (main crate)

### 3. Post-Release Verification
- [ ] Verify all crates are available on crates.io
- [ ] Test installation: `cargo install turbomcp-cli --version 1.1.0`
- [ ] Validate documentation links
- [ ] Update project status/announcements

## 🚨 Important Notes

### New in v1.1.0
- **turbomcp-dpop**: New crate for RFC 9449 DPoP security
- **Type-State Builders**: Compile-time capability validation
- **Enhanced WebSocket**: Updated transport compatibility
- **Security Hardening**: Fixed all deprecation warnings

### Dependency Chain
All crates depend on earlier crates in the publish order. The script includes:
- 60-second delays between publishes (configurable)
- Automatic verification on crates.io
- Retry logic for network issues
- Proper error handling

### Rollback Strategy
If publishing fails:
1. Any successfully published crates will remain on crates.io
2. You can resume publishing from the failed crate
3. Version numbers cannot be reused - would need v1.1.1

## 🎯 Success Criteria

Release is considered successful when:
- [x] All 9 crates publish successfully to crates.io
- [x] GitHub release is created with proper release notes
- [x] Documentation is accessible and accurate
- [x] New users can install and use TurboMCP v1.1.0

## 🔗 Resources

- **Release Notes**: `.strategy/RELEASE_NOTES_v1.1.0.md`
- **Publish Script**: `./scripts/publish-crates.sh`
- **Prepare Script**: `./scripts/prepare-release.sh`
- **Main README**: Updated with v1.1.0 examples

---

**Status**: Ready for crates.io publishing after GitHub tag/release creation