//! V8 snapshot support for fast startup
//!
//! Note: Snapshot creation in deno_core requires using `deno_core::snapshot::create_snapshot()`
//! in build.rs with extensions. For now, we support loading existing snapshots only.

use std::path::PathBuf;

/// V8 snapshot configuration
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Path to snapshot file
    pub path: PathBuf,
}

impl SnapshotConfig {
    /// Create a new snapshot config
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

/// Snapshot manager for loading V8 snapshots
pub struct SnapshotManager {
    config: SnapshotConfig,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(config: SnapshotConfig) -> Self {
        Self { config }
    }

    /// Check if snapshot exists
    pub fn exists(&self) -> bool {
        self.config.path.exists()
    }

    /// Load existing snapshot from disk
    pub fn load(&self) -> crate::Result<Vec<u8>> {
        if !self.exists() {
            return Err(anyhow::anyhow!(
                "Snapshot not found at {}",
                self.config.path.display()
            ));
        }

        let snapshot = std::fs::read(&self.config.path)?;

        tracing::info!(
            path = %self.config.path.display(),
            size_kb = snapshot.len() / 1024,
            "Snapshot loaded"
        );

        Ok(snapshot)
    }

    /// Get snapshot if it exists
    pub fn get_or_create(&self) -> crate::Result<Option<Vec<u8>>> {
        if self.exists() {
            Ok(Some(self.load()?))
        } else {
            Ok(None)
        }
    }

    /// Delete snapshot file
    pub fn delete(&self) -> crate::Result<()> {
        if self.exists() {
            std::fs::remove_file(&self.config.path)?;
            tracing::info!(
                path = %self.config.path.display(),
                "Snapshot deleted"
            );
        }
        Ok(())
    }
}

/// Builder for snapshot configurations
pub struct SnapshotBuilder {
    path: PathBuf,
}

impl SnapshotBuilder {
    /// Create a new snapshot builder with path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Build the snapshot configuration
    pub fn build(self) -> SnapshotConfig {
        SnapshotConfig { path: self.path }
    }
}
