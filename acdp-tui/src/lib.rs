mod acme;
mod activity_feed;
pub mod app;
mod client_proxy;
mod clients_panel;
pub mod colors;
pub mod components;
mod config;
mod diagnostics;
pub mod events;
mod http_server;
pub mod ipc_handler;
mod llm_responses_panel;
mod acdp_server;
mod query_input;
mod quick_access;
mod semantic_status;
mod servers_panel;
mod settings_panel;
mod status_bar;
mod tabs;
pub mod ui;

// Re-export key types for external use
pub use app::App;
pub use components::*;
pub use events::{Event, EventHandler};
pub use ipc_handler::{AppStateUpdate, IpcHandler};

use anyhow::Result;
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the TUI application
pub async fn init() -> Result<App> {
    App::new().await
}

/// Arguments for running the monitor from other binaries (e.g., mcp-cli)
pub struct MonitorArgs {
    pub ipc_socket: String,
    pub verbose: bool,
}

/// Launch the TUI.
pub async fn launch_tui() -> Result<()> {
    // FIRST: Suppress stderr for LiteRT C++ logs (before any LLM initialization)
    // This prevents LiteRT logs from appearing on terminal
    // Always suppress if we're in a TUI environment since LiteRT may be used
    let _stderr_guard = unsafe {
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("litert.log")
            .ok();

        if let Some(file) = log_file {
            let original_stderr = libc::dup(libc::STDERR_FILENO);
            libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO);
            Some((original_stderr, file))
        } else {
            None
        }
    };

    // Initialize logging to file only (stderr is redirected for LiteRT)
    let file_appender = tracing_appender::rolling::never(".", "out.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting MCP TUI - Next Generation Interface");

    let start = std::time::Instant::now();
    info!("[STARTUP] Begin initialization at {:?}", start);

    // Initialize the application
    info!("[STARTUP] Creating App instance...");
    let app_start = std::time::Instant::now();
    let mut app = App::new().await?;
    info!(
        "[STARTUP] App::new() completed in {:?}",
        app_start.elapsed()
    );
    info!("[STARTUP] Total startup time: {:?}", start.elapsed());

    // Run the TUI
    info!("[STARTUP] Entering TUI event loop");
    if let Err(e) = app.run().await {
        error!("Application error: {}", e);

        // Restore stderr before printing error
        if let Some((original_stderr, _)) = _stderr_guard {
            unsafe {
                libc::dup2(original_stderr, libc::STDERR_FILENO);
                libc::close(original_stderr);
            }
        }

        eprintln!("Application error: {}", e);
        return Err(e);
    }

    info!("MCP TUI shutdown complete");
    Ok(())
}

/// Entry point used by external binaries to start the monitor-only mode.
pub async fn run_monitor_app(args: MonitorArgs) -> Result<()> {
    std::env::set_var("MCP_IPC_SOCKET", &args.ipc_socket);
    if args.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }

    launch_tui().await
}
