# TUI Redesign Plan

## Current Layout
```
┌─────────────────┬─────────────────┐
│   Clients (50%) │  Activity (40%) │
├─────────────────┤                 │
│   Servers (50%) ├─────────────────┤
│                 │  Quick (30%)    │
│                 ├─────────────────┤
│                 │  Diag (30%)     │
└─────────────────┴─────────────────┘
│         Query Input               │
└───────────────────────────────────┘
```

## New Layout
```
┌──────────────────────────────────────────────┐
│  Status Bar (horizontal metrics)             │
├──────┬───────────────────────────────────────┤
│Server│  Activity Feed                        │
│(25ch)│                                        │
│      ├───────────────────────────────────────┤
│      │  Quick Actions                        │
└──────┴───────────────────────────────────────┘
│  Semantic Status (query → prediction)        │  ← Only when semantic routing
├──────────────────────────────────────────────┤
│  Query Input                                 │
└──────────────────────────────────────────────┘
```

## Changes

1. **Top Status Bar** - Horizontal display of:
   - Model status (Ready/Loading/Error)
   - Routing mode (🔓 bypass / 🧠 semantic / ⚡ hybrid)
   - TTFT, Tokens/sec
   - Session accuracy
   - Prediction count

2. **Left Panel** - Servers Only (25 chars wide)
   - Shows proxy list with routing modes
   - Selected proxy marked with ►
   - Narrow width to maximize activity space

3. **Right Panels** - Activity + Quick Actions
   - Activity feed gets more vertical space
   - Quick actions below

4. **Semantic Status Bar** - Conditionally shown
   - Only appears when semantic routing is active
   - Left: "Query: Check the health status..."
   - Right: "Predicted: resources/list (85%) ✓"
   - Shows success/failure with ✓/✗

5. **Clients Panel** - Hidden by default
   - Type `/clients` in query input to toggle visibility
   - When shown, replaces servers panel temporarily
   - Type `/servers` or Esc to go back

## Implementation Steps

1. ✅ Create `status_bar.rs` - horizontal metrics
2. ✅ Create `semantic_status.rs` - prediction display
3. ⏳ Update `ui.rs` layout logic
4. ⏳ Add `/clients` command handler in `app.rs`
5. ⏳ Track last semantic prediction in app state
6. ⏳ Update navigation to work with new layout
7. ⏳ Test and adjust widths/heights

## New Focus Order

```
StatusBar (read-only) → Servers → Activity → QuickAccess → QueryInput
```

Clients panel accessible via `/clients` command only.
