# End-to-End Integration Tests

This directory contains comprehensive end-to-end tests for the MCP system.

## Test Files

### `full_system_tests.rs`
Tests IPC message flow between components:
- Proxy lifecycle updates
- Client and session management
- Message routing and stats

### `multi_proxy_routing_test.rs`
**NEW**: Comprehensive multi-proxy routing mode tests including:
- Multiple proxy instances with independent routing modes
- Routing mode validation (bypass, semantic, hybrid)
- Invalid routing mode rejection
- Proxy lifecycle with routing changes

## Test Types

### Unit-Level Tests (Always Run)
```bash
cargo test -p assist-mcp-tests
```

These tests verify:
- ✅ `test_routing_mode_validation` - Validates routing mode strings
- ✅ `test_proxy_lifecycle_with_routing` - Simulates proxy lifecycle with mode changes

### Full Integration Tests (Ignored by Default)
```bash
cargo test -p assist-mcp-tests -- --ignored
```

or use the helper script:
```bash
./test_e2e_multi_proxy.sh
```

These tests:
- ✅ `test_multi_proxy_routing_modes` - Spawns real processes (2 proxies, 2 servers)
  - Verifies routing mode changes across multiple proxies
  - Tests invalid mode handling
  - Validates proxy selection logic

## Running the Full E2E Test

The full integration test (`test_multi_proxy_routing_modes`) is marked `#[ignore]` because it:
1. Requires binaries to be built (`mcp-cli`)
2. Spawns multiple system processes
3. Takes longer to run (~30s+)
4. May interfere with other running tests

### Prerequisites
```bash
# Build the binaries first
cargo build --bin mcp-cli --bin mcp-tui

# Ensure Python 3 is available (for test servers)
python3 --version
```

### Run Full Test
```bash
# Option 1: Use the helper script (recommended)
./test_e2e_multi_proxy.sh

# Option 2: Run directly with cargo
RUST_LOG=info cargo test -p assist-mcp-tests test_multi_proxy_routing_modes -- --ignored --nocapture
```

## Test Architecture

### `E2ETestFixture`
A test fixture that manages spawned processes and provides helpers for:
- Spawning MCP proxies
- Spawning Python test servers
- Sending routing mode change requests
- Cleaning up processes on test completion

### Process Management
- All spawned processes are tracked in `ManagedProcess` structs
- Automatic cleanup on test failure via Drop trait
- Graceful shutdown via `cleanup()` method

### What Gets Tested

**Multi-Proxy Scenario:**
```
┌─────────────────────────────────────┐
│         IPC Monitor                  │
│      (Simulated TUI Backend)         │
└────────────┬────────────────────────┘
             │
        ┌────┴──────┐
        │           │
   ┌────▼───┐  ┌───▼────┐
   │Proxy 1 │  │Proxy 2 │
   │(port A)│  │(port B)│
   └────┬───┘  └───┬────┘
        │          │
   ┌────▼───┐ ┌───▼────┐
   │Python  │ │Python  │
   │Server 1│ │Server 2│
   └────────┘ └────────┘
```

**Test Flow:**
1. Spawn two proxies with different backend servers
2. Change Proxy 1 to "semantic" mode
3. Change Proxy 2 to "hybrid" mode
4. Attempt invalid mode change (should be rejected)
5. Change Proxy 1 back to "bypass" mode
6. Verify all changes via IPC messages
7. Clean up all processes

## Adding New E2E Tests

1. Create your test function in `multi_proxy_routing_test.rs` or a new file
2. Use `#[tokio::test]` for async tests
3. Add `#[ignore]` if the test spawns processes or takes >5 seconds
4. Use `E2ETestFixture::new()` for managed process spawning
5. Document expected behavior in comments

Example:
```rust
#[tokio::test]
#[ignore] // Spawns processes
async fn test_my_e2e_scenario() -> Result<()> {
    let mut fixture = E2ETestFixture::new().await?;

    // Your test logic here
    let proxy_id = fixture.spawn_proxy("my-proxy", 0).await?;
    fixture.change_routing_mode(proxy_id, "semantic").await?;

    fixture.cleanup().await;
    Ok(())
}
```

## Troubleshooting

### "Failed to spawn proxy"
- Ensure `mcp-cli` binary is built: `cargo build --bin mcp-cli`
- Check that the current directory is the repository root

### "Python test server not found"
- Ensure `tests/common/test_server.py` exists
- Check Python 3 is installed: `python3 --version`

### Tests hang or timeout
- Check for orphaned processes: `ps aux | grep mcp-cli`
- Kill manually if needed: `pkill -f mcp-cli`
- Increase timeout in test code if on slow systems

### Port conflicts
- Tests use dynamic ports allocated by the OS
- If you see "Address already in use", wait a few seconds and retry

## CI/CD Integration

For CI pipelines, run only the fast unit tests:
```bash
# Fast tests (< 1 second)
cargo test -p assist-mcp-tests

# Skip ignored integration tests in CI (unless on main branch)
```

For release validation, run the full suite:
```bash
# Full test suite including E2E
./test_e2e_multi_proxy.sh
```
