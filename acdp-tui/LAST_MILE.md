# LAST_MILE

Places where the current TUI still relies on placeholder data or stubs instead of real MCP gateway wiring:

- `src/app.rs:102` – `init_sample_data` seeds demo clients/servers/activities. Replace with a gateway call (or subscription) that pulls the live registry of MCP clients and servers, plus any existing activity log.
- `src/app.rs:118` – `process_query` only appends a synthetic “Processing” entry and returns. Call into the proxy to submit the query, capture the response/error, and push the resulting activity update back into `self.activities`.
- `src/app.rs:148` – Quick Access handling just echoes the static command string and marks it successful. Map each quick action to a concrete gateway operation (e.g., “list_tools”, “check_health”), execute it, and write the real outcome to the feed.
- `src/app.rs:178` – `update_state` currently just trims the feed. Poll or subscribe to gateway state changes (client/server status updates, new activities, etc.) and merge them here; keep the truncation as a guard.

Additional integration work:

1. Define canonical conversion helpers that translate real MCP client/server/activity structs into the lightweight `components::*` models so the UI stays decoupled.
2. Decide how activity updates arrive (push via streaming events, pull via polling) and extend `App::run` to ingest those updates without blocking the UI.
3. Once the live data exists, remove or gate the demo seeding so production builds don’t show sample content.
