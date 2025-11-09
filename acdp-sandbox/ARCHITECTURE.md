# mcp-sandbox Architecture

## Overview

Flexible, extensible sandbox service for executing untrusted code across multiple runtime environments.

## Core Design

```
SandboxService
    ↓
Runtime Trait (simple interface)
    ↓
┌──────────────┬──────────────┬──────────────┬──────────────┐
│   V8/Hono    │  Containers  │  OS Sandbox  │ QEMU/WASM    │
│   Runtime    │   Runtime    │   Runtime    │   Runtime    │
└──────────────┴──────────────┴──────────────┴──────────────┘
```

## Runtime Priority

### Phase 1: Off-the-Shelf (Current Priority)

**V8/Hono Runtime**
- Execute JavaScript/TypeScript in V8 isolate
- Hono-style worker environments
- Fast startup, low overhead
- Built-in security boundaries

**Container Runtime**
- Docker/Podman-based execution
- Full OS isolation
- Resource limits (cgroups)
- Network isolation

**OS-Level Sandbox**
- **macOS**: `sandbox-exec` with custom profiles
- **Linux**: `landlock` LSM for fine-grained restrictions
- Process-level isolation
- Filesystem/network ACLs

### Phase 2: Custom QEMU

**QEMU MTTCG Runtime**
- Emscripten-compiled QEMU
- Multi-threaded tiny code generator
- Pre-configured WASM runtimes
- Full system emulation for maximum isolation

### Phase 3: Service Layer (Future)

**Attestation Service**
- Runtime environment verification
- Code signing validation
- Trust chain verification

**Scanning Service**
- Static analysis pre-execution
- Malware detection
- Vulnerability scanning

## Runtime Interface

All runtimes implement:

```rust
#[async_trait]
trait Runtime: Send + Sync {
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionStream>;
    fn name(&self) -> &str;
}
```

**ExecutionStream provides:**
- Streaming stdout/stderr
- Final execution result
- Duration/timeout tracking

## Design Principles

1. **Simple Base** - One trait, minimal coupling
2. **Extensible** - Add runtimes without changing core
3. **Runtime-Agnostic** - Same API for all backends
4. **Streaming First** - Real-time output for long-running code
5. **No Dependencies on mcp-llm** - Only uses mcp-common

## Current Implementation Status

- ✅ Core trait and service
- ✅ Process runtime (baseline)
- ✅ WASM runtime (placeholder)
- ⏳ V8/Hono runtime (next)
- ⏳ Container runtime
- ⏳ OS sandbox runtime
- ⏳ QEMU runtime

## Remote Execution (Future)

The same Runtime trait will extend to remote environments:
- Browser (WebAssembly)
- Mobile (iOS/Android)
- Edge workers (Cloudflare, etc.)

Remote runtimes implement the same interface, with network transport handled transparently.
