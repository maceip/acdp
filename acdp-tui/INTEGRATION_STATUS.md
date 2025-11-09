# TUI Integration Status (Sorted by User Importance)

## Summary

The TUI has structures in place for real integrations but many are **not connected** to actual gateway/services. There are also several **pseudo/mock implementations** used for demo purposes.

**Priority Guide**: 🔴 Critical → 🟠 High → 🟡 Medium → 🔵 Low

## Quick Action Plan (Fix Order)

**🔴 Must Fix First (Core Functionality):**
1. **Gateway Client Connection** (#4) - Foundation for everything
2. **Query Processing** (#1) - Users expect queries to work
3. **Client/Server Data** (#2) - Show real infrastructure, not fakes
4. **Activity Log** (#3) - Show real operations, not synthetic entries

**🟠 Fix Next (Key Features):**
5. **Quick Access Actions** (#5) - Make buttons actually work
6. **MCP Servers Discovery** (#6) - Proper server management

**🟡 Fix Later (Polish):**
7. **State Updates** (#7) - Real-time refresh
8. **Diagnostics Metrics** (#8) - Real performance numbers
9. **Proxy Sessions** (#9) - Backend monitoring

**🔵 Cleanup (After integration works):**
- Remove/gate sample data
- Disable PseudoDemo in production
- Fix merge conflict in `pseudo_server.rs`

---

## 🔴 CRITICAL - Core Functionality Broken

These issues directly impact what users see and do. Fix these first for a usable product.

### 1. **Query Processing** (`process_query()`) - 🔴 MOST IMPORTANT
   - **User Impact**: Users type queries and expect real results through the MCP gateway
   - **Location**: `src/app.rs:263`
   - **Status**: Bypasses gateway entirely, calls LLM directly
   - **Problem**: 
     - Queries don't route through MCP gateway/proxy
     - No proxy session management
     - Activity feed shows fake/synthetic entries
     - Users can't use the TUI for its intended purpose
   - **Files**: `src/app.rs:263-337`

### 2. **Client/Server Data** (`clients`, `servers`) - 🔴 CRITICAL
   - **User Impact**: TUI shows fake clients and servers instead of real ones
   - **Location**: `src/app.rs:31-33, 105-106, 381-450`
   - **Status**: Populated with **hardcoded sample data**
   - **Problem**: 
     - `init_sample_data()` creates fake "ai-assistant", "code-editor", "python-server", "database"
     - Real clients/servers from gateway never loaded
     - Users see demo data, not their actual infrastructure
   - **Files**: `src/app.rs:381-450` (`init_sample_data()`)

### 3. **Activity Log Feed** - 🔴 CRITICAL
   - **User Impact**: Activity feed shows fake/synthetic activities, not real MCP operations
   - **Location**: `src/app.rs:35, 49, 115`
   - **Status**: Uses synthetic `activities: Vec<ActivityItem>`, ignores `activity_log: Vec<LogEntry>`
   - **Problem**: 
     - Users see fake activity from `init_sample_data()`
     - Real gateway activity log never fetched
     - Query responses don't populate activity feed correctly
   - **Files**: `src/app.rs:381-450`, `src/app.rs:263-337`

---

## 🟠 HIGH - Blocks Important Features

These prevent users from using key features of the TUI.

### 4. **MCP Gateway Client** (`gateway_client`) - 🟠 HIGH
   - **User Impact**: Foundation for all real integrations - nothing works without this
   - **Location**: `src/app.rs:45, 113`
   - **Status**: Field exists but always `None`, never initialized
   - **Problem**: 
     - No connection to MCP gateway/proxy
     - Blocks all other integrations
     - Must be fixed before any real functionality works
   - **Files**: `src/app.rs:66-126` (`App::new()`)

### 5. **Quick Access Actions** - 🟠 HIGH
   - **User Impact**: Quick action buttons don't do anything real
   - **Location**: `src/app.rs:231-243`
   - **Status**: Just echoes action name and marks as successful
   - **Problem**: 
     - `list_tools` doesn't actually list tools
     - `check_health` doesn't check health
     - Users click buttons expecting real operations
   - **Files**: `src/app.rs:231-243`, `src/ui.rs` (quick_access)

### 6. **MCP Servers Discovery** (`mcp_servers`) - 🟠 HIGH
   - **User Impact**: Can't see or manage real MCP servers
   - **Location**: `src/app.rs:51, 116`
   - **Status**: Empty HashMap, never populated from gateway
   - **Problem**: 
     - Should discover/register servers from gateway
     - Currently shows fake servers from sample data
   - **Files**: `src/app.rs:381-450`, `src/app.rs:340-379`

---

## 🟡 MEDIUM - Monitoring & Debugging

Important for power users monitoring system health, but not blocking basic use.

### 7. **Real-time State Updates** (`update_state()`) - 🟡 MEDIUM
   - **User Impact**: Status panels don't update with real gateway state
   - **Location**: `src/app.rs:340-379`
   - **Status**: Only updates model manager, has hardcoded placeholders
   - **Problem**: 
     - No polling/subscription to gateway state changes
     - Client/server status never refreshes
     - New activities from gateway not fetched
   - **Files**: `src/app.rs:340-379`

### 8. **Diagnostics Metrics** (GEPA/DSPy) - 🟡 MEDIUM
   - **User Impact**: Diagnostics show fake performance numbers
   - **Location**: `src/app.rs:371-372`, `src/diagnostics.rs`
   - **Status**: Hardcoded placeholders (`0.15` and `0.85`)
   - **Problem**: 
     - GEPA optimization always shows 15%
     - DSPy accuracy always shows 85%
     - Users making decisions based on fake data
   - **Files**: `src/app.rs:366-373`, `src/diagnostics.rs`

### 9. **Proxy Sessions Tracking** (`proxy_sessions`) - 🟡 MEDIUM
   - **User Impact**: Can't monitor active proxy sessions (backend concern)
   - **Location**: `src/app.rs:47, 114`
   - **Status**: Empty HashMap, never populated
   - **Problem**: 
     - Should track active sessions for monitoring
     - Less visible to end users, but important for debugging
   - **Files**: `src/app.rs:47, 114`

---

## 🔵 LOW - Mocked/Pseudo Components (Demo/Testing)

These are intentionally mocked for demo purposes. Can be removed/gated after real integrations are complete.

### 1. **Sample Data** (`init_sample_data()`) - 🔵 LOW PRIORITY
   - **User Impact**: None by default. Sample entries only appear when demo mode is enabled.
   - **Location**: `src/app.rs`
   - **Status**: ✅ Gated behind the `MCP_TUI_ENABLE_DEMO=1` env var (defaults to disabled)
   - **Purpose**: Demo/placeholder content for screenshots or offline demos
   - **Action**: Leave disabled in production; enable via env var when needed

### 2. **PseudoDemo** (`pseudo_demo`) - 🔵 LOW PRIORITY
   - **User Impact**: None by default. Background simulator only runs in demo mode.
   - **Location**: `src/pseudo_demo.rs`, `src/app.rs`
   - **Status**: ✅ Only started when `MCP_TUI_ENABLE_DEMO` is truthy
   - **Purpose**: Demo/testing only
   - **Action**: Keep off in production; enable for mock walkthroughs when useful

### 3. **PseudoMcpClient** - 🔵 LOW PRIORITY
   - **User Impact**: None currently (only used by PseudoDemo)
   - **Location**: `src/pseudo_client.rs`
   - **Status**: Fully implemented mock client with `simulate_connection()` and `simulate_response()`
   - **Action**: Keep for testing, ensure not used in production path

### 4. **PseudoMcpServer** - 🔵 LOW PRIORITY  
   - **User Impact**: None currently (only used by PseudoDemo)
   - **Location**: `src/pseudo_server.rs`
   - **Note**: ⚠️ **File has merge conflict markers** (lines 1-468) - must fix!
   - **Status**: Mock server implementation
   - **Action**: Resolve merge conflict, keep for testing only

---

## ✅ Working Integrations

### 1. **LLM Service Facade** (`mcp_llm::LlmService`)
   - **Location**: `mcp-llm/src/service.rs`, `mcp-tui/src/app.rs`
   - **Status**: ✅ **Fully integrated**
   - **Features**:
     - Wraps the model manager and streaming helpers behind a stable API
     - Background initialization task ensures models are available + warmed
     - Service broadcasts model status / download progress / generation metrics
     - TUI subscribes to those events and updates diagnostics accordingly

### 2. **LLM Query Processing**
   - **Location**: `src/app.rs:278-336`
   - **Status**: ✅ **Partially integrated**
   - **Working**: 
     - Creates real LLM sessions
     - Generates streaming responses
     - Updates activity feed with real responses
   - **Missing**: Should route through gateway/proxy instead of direct LLM calls

### 3. **Diagnostics Panel**
   - **Location**: `src/diagnostics.rs`, `src/app.rs:59, 122`
   - **Status**: ✅ **Partially integrated**
   - **Working**:
     - Model status (real)
     - Download progress (real)
     - Model name (real)
   - **Mocked**:
     - GEPA optimization % (placeholder)
     - DSPy accuracy % (placeholder)
     - TTFT/tokens_per_sec (not populated)

---

## 🔌 How to Connect

### Gateway Client Connection
```rust
// In App::new(), after config load:
let gateway_config = TransportConfig::stdio(
    "path/to/gateway", 
    &["--socket", "/tmp/gateway.sock"]
);
let gateway_client = Arc::new(
    McpClient::with_defaults(gateway_config).await?
);
gateway_client.connect(client_info).await?;
```

### Fetching Real Data
```rust
// Replace init_sample_data() with:
async fn load_real_data(&mut self) {
    if let Some(ref gateway) = self.gateway_client {
        // Fetch clients/servers from gateway registry
        // Fetch activity log
        // Fetch proxy sessions
    }
}
```

### Subscribing to Updates
```rust
// In update_state(), add:
if let Some(ref gateway) = self.gateway_client {
    // Poll gateway for:
    // - Client/server status updates
    // - New activity log entries
    // - Session statistics for diagnostics
}
```

### Query Routing
```rust
// In process_query(), route through gateway:
if let Some(ref gateway) = self.gateway_client {
    // Create proxy session
    // Submit query through gateway
    // Capture streaming response
    // Update activity feed
}
```

---

## 📝 Related Files

- `LAST_MILE.md` - Additional integration notes
- `src/app.rs` - Main application logic with integration points
- `src/components.rs` - UI component models
- `src/pseudo_*.rs` - Mock implementations (can be removed after integration)
