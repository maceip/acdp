## Overview

<img  align="left" width="170" height="170" alt="Zim_V2p" src="https://github.com/user-attachments/assets/e029a276-af9c-435d-b9f0-8dede55f8f7b" />    The race for reliable, autonomous agents will be won by a new "core-llm" paradigm of automatic optimization, built on fair, verifiable primitives, not last-gen trust models. Our vision is a technical stack and governance model for continuous one-shot optimization, secured by an open trust fabric: the Agent Credential Delegation Protocol (ACDP). At its core, ACDP enables the self-issuance of verifiable, cryptographic capabilities, allowing humans and agents to autonomously delegate power. This solves trust and control with privacy-preserving, unlinkable anonymous credentials and is built to interoperate with existing and emergent protocols like OAuth2, X402, ACP, and WebAuthn.<br />

---
### Crates


**[acdp-core](./acdp-core)** <br />
<dl>
  <dd>

<sup>Transport-agnostic codec with pluggable wire formats e.g., stdio, HTTP+SSE, streaming). Async client with connection lifecycle management.</sup><br />

_The foundational protocol implementation. Provides universal transport abstractions and client primitives, establishing the common language for agent communication over any channel._
</dd>
</dl>

**[acdp-common](./acdp-common)** <br />
<dl>
  <dd>

<sup>Serde message envelopes, Unix socket IPC, shared command abstraction layer for CLI/TUI reuse.</sup><br />

_The canonical dictionary for the agentic ecosystem. Defines the shared, versioned serialization contracts, ensuring all components speak the same, unambiguous language._
</dd>
</dl>

**[acdp-transport](./acdp-transport)** <br />
<dl>
  <dd>

<sup>Interceptor middleware chain. Backend connection pool routing stdio↔HTTP↔IPC with buffered message handling.</sup><br />

_The intelligent gateway realized. An advanced proxy server that moves beyond "dumb pipes" with live-reloadable hooks for traffic transformation, forming the active nexus of the agent network._
</dd>
</dl>

**[acdp-llm](./acdp-llm)** <br />
<dl>
  <dd>

<sup>Rust FFI to TensorFlow LiteRT for on-device inference. DSPy-RS for structured predictions. SQLite for execution traces and GEPA optimization state.</sup><br />

_The "LLM-at-the-core" engine. It embeds on-device inference and DSPy-style optimization directly into the gateway, enabling the one-shot task completion that defines our 24-month technical lead._
</dd>
</dl>

**[acdp-tui](./acdp-tui)** <br />
<dl>
  <dd>

<sup>Ratatui event loop with client proxy managing MCP backends. Axum HTTP+SSE server for remote access, rustls-acme for auto-TLS.</sup><br />

_The network's command center. A real-time, multiplexed dashboard providing complete observability. It moves beyond logs to offer deep insight into LLM optimization, with auto-TLS for production readiness._
</dd>
</dl>

**[acdp-cli](./acdp-cli)** <br />
<dl>
  <dd>

<sup>Thin wrapper forwarding args to acdp-transport or acdp-tui based on subcommand.</sup><br />

_The unified entrypoint for the entire stack. A single, powerful binary composing all components for a seamless dev/agent experience, from local proxying to full monitoring._
</dd>
</dl>

**[acdp-auth](./acdp-auth)** <br />
<dl>
  <dd>

<sup>Actix-web gateway implementing ACDP credential lifecycle. Ed25519 for identity-bound sigs, P256 hash-to-curve for ARC, sigma-proofs for ZK range verification.</sup><br />

_The cryptographic heart of the open trust fabric. This reference IAM implementation provides self-issuing capabilities, delegation chains, and sub-750µs unlinkable anonymous credentials (ARC)._
</dd>
</dl>

**[acdp-sandbox](./acdp-sandbox)** <br />
<dl>
  <dd>

<sup>Runtime trait with process/WASM (Wasmtime)/V8 (deno_core) backends. LLM code generator feeding execution service with resource limit enforcement.</sup><br />

_The verifiable containment field for an autonomous world. Provides isolated, resource-limited execution contexts for untrusted tools, ensuring capability is a verifiably enforced boundary._
</dd>
</dl>

**[tests](./tests)** <br />
<dl>
  <dd>

<sup>Shared test harness with Python MCP servers, multi-proxy routing fixtures, stdio↔HTTP bridge validation, DSPy assertion helpers.</sup><br />

_The comprehensive validation harness proving the stack's resilience. This full-stack E2E harness validates everything from transport bridges to LLM optimization assertions, ensuring all protocol guarantees are met._
</dd>
</dl>
