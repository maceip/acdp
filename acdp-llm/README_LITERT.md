# MCP-LLM with LiteRT-LM Integration

This crate provides Rust bindings for LiteRT-LM, enabling local LLM inference with conversation management.

## Architecture

- **build.rs**: Links against the LiteRT-LM Rust API dynamic library (`liblitert_lm_rust_api.so`)
- **litert_wrapper.rs**: Safe Rust FFI wrapper around the C API
- **examples/chat.rs**: Interactive chat example demonstrating conversation capabilities

## Setup

### Prerequisites

1. Build LiteRT-LM with Bazel:
   ```bash
   cd ~/LiteRT-LM
   bazel build //rust_api:litert_lm_rust_api
   ```

2. Download a LiteRT-LM model (`.litertlm` file)

### Running

Set the library path and run the chat example:

```bash
DYLD_LIBRARY_PATH=~/LiteRT-LM/bazel-bin/rust_api cargo run --example chat <path/to/model.litertlm>
```

Or use the convenience script:

```bash
./run_chat.sh
```

## Features

- **Clean FFI**: Direct Rust bindings to LiteRT-LM C API
- **Session Management**: Conversation-aware API using LiteRT's Conversation backend
- **Thread-Safe**: Engine and Session types implement Send + Sync
- **Memory Safe**: Proper RAII cleanup with Drop implementations

## API Usage

```rust
use mcp_llm::{LiteRTEngine, LiteRTBackend};

// Create engine with model
let engine = LiteRTEngine::new("model.litertlm", LiteRTBackend::Cpu)?;

// Create conversation session
let session = engine.create_session()?;

// Generate responses
let response = session.generate("What is 2+2?")?;
println!("Response: {}", response);
```

## Testing

The chat example has been tested with Gemma 3N models and correctly handles:
- Multi-turn conversations
- Context awareness
- Tool call formatting (via model templates)
- Graceful shutdown

## Implementation Notes

This implementation uses the simpler approach from `/Users/rpm/mcp-llm-litert-rs`:
- Links dynamically against the Rust API shared library
- Uses DYLD_LIBRARY_PATH at runtime to locate dependencies
- Avoids the complexity of static linking all C++ dependencies
- Maintains compatibility with LiteRT-LM's Bazel build system
