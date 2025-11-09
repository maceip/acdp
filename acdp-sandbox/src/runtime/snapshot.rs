//! V8 snapshot support for fast startup

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// V8 snapshot configuration
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Path to snapshot file
    pub path: PathBuf,
    /// Whether to create snapshot if it doesn't exist
    pub auto_create: bool,
    /// Initialization code to include in snapshot
    pub init_code: Vec<String>,
}

impl SnapshotConfig {
    /// Create a new snapshot config
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            auto_create: false,
            init_code: Vec::new(),
        }
    }

    /// Enable automatic snapshot creation
    pub fn with_auto_create(mut self) -> Self {
        self.auto_create = true;
        self
    }

    /// Add initialization code to snapshot
    pub fn with_init_code(mut self, code: impl Into<String>) -> Self {
        self.init_code.push(code.into());
        self
    }

    /// Add multiple init code snippets
    pub fn with_init_codes<I, S>(mut self, codes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.init_code.extend(codes.into_iter().map(|s| s.into()));
        self
    }
}

/// Snapshot manager for creating and loading V8 snapshots
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

    /// Create a new snapshot with the configured initialization code
    pub fn create(&self) -> crate::Result<Vec<u8>> {
        use deno_core::{JsRuntime, RuntimeOptions};

        tracing::info!(
            path = %self.config.path.display(),
            init_scripts = self.config.init_code.len(),
            "Creating V8 snapshot"
        );

        // Create a temporary runtime for snapshot creation
        let mut runtime = JsRuntime::new(RuntimeOptions {
            will_snapshot: true,
            ..Default::default()
        });

        // Execute all initialization code
        for (idx, code) in self.config.init_code.iter().enumerate() {
            let name = format!("<snapshot-init-{}>", idx);
            runtime
                .execute_script(&name, code.clone())
                .map_err(|e| anyhow::anyhow!("Failed to execute init code {}: {}", idx, e))?;
        }

        // Create the snapshot
        let snapshot = runtime
            .snapshot()
            .map_err(|e| anyhow::anyhow!("Failed to create snapshot: {}", e))?;

        // Save to file if path is set
        std::fs::write(&self.config.path, &snapshot)?;

        tracing::info!(
            path = %self.config.path.display(),
            size_kb = snapshot.len() / 1024,
            "Snapshot created successfully"
        );

        Ok(snapshot)
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

    /// Get or create snapshot (creates if auto_create is enabled)
    pub fn get_or_create(&self) -> crate::Result<Option<Vec<u8>>> {
        if self.exists() {
            Ok(Some(self.load()?))
        } else if self.config.auto_create {
            Ok(Some(self.create()?))
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

/// Builder for common snapshot configurations
pub struct SnapshotBuilder {
    path: PathBuf,
    auto_create: bool,
    init_code: Vec<String>,
}

impl SnapshotBuilder {
    /// Create a new snapshot builder with path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            auto_create: false,
            init_code: Vec::new(),
        }
    }

    /// Enable auto-creation
    pub fn auto_create(mut self) -> Self {
        self.auto_create = true;
        self
    }

    /// Add standard library polyfills
    pub fn with_stdlib(mut self) -> Self {
        // Common utilities that benefit from being in snapshot
        self.init_code.push(
            r#"
            // Console stub for sandbox
            globalThis.console = globalThis.console || {
                log: (...args) => {},
                error: (...args) => {},
                warn: (...args) => {},
                info: (...args) => {},
            };
            "#
            .to_string(),
        );
        self
    }

    /// Add custom initialization code
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.init_code.push(code.into());
        self
    }

    /// Build the snapshot configuration
    pub fn build(self) -> SnapshotConfig {
        SnapshotConfig {
            path: self.path,
            auto_create: self.auto_create,
            init_code: self.init_code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_config() {
        let config = SnapshotBuilder::new("/tmp/test.snap")
            .auto_create()
            .with_stdlib()
            .with_code("const x = 42;")
            .build();

        assert_eq!(config.path, PathBuf::from("/tmp/test.snap"));
        assert!(config.auto_create);
        assert_eq!(config.init_code.len(), 2);
    }
}
