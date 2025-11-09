use anyhow::Result;
use acdp_tui::launch_tui;

#[tokio::main]
async fn main() -> Result<()> {
    launch_tui().await
}
