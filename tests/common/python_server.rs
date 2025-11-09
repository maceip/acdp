//! Python MCP test server utilities

use anyhow::Result;
use std::path::Path;
use std::process::{Child, Command};

/// Start the Python test MCP server
pub fn start_python_server() -> Result<Child> {
    let server_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("common")
        .join("test_server.py");

    Command::new("python3")
        .arg(&server_path)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start Python server: {}", e))
}

/// Stop the Python test MCP server
pub fn stop_python_server(mut child: Child) -> Result<()> {
    child.kill()?;
    Ok(())
}
