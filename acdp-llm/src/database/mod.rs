//! Database schema and main database struct

use crate::error::LlmResult;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::str::FromStr;

pub mod gepa;
pub mod metrics;
pub mod predictions;
pub mod routing_rules;
pub mod schema;

pub use gepa::{GepaDatabase, GepaOptimizationRecord};
pub use metrics::MetricsDatabase;
pub use predictions::{AccuracyMetrics, PredictionsDatabase};
pub use routing_rules::{RoutingRule, RoutingRulesDatabase};

/// Main LLM database coordinator
pub struct LlmDatabase {
    pub pool: SqlitePool,
    pub routing_rules: RoutingRulesDatabase,
    pub predictions: PredictionsDatabase,
    pub metrics: MetricsDatabase,
    pub gepa: GepaDatabase,
}

impl LlmDatabase {
    /// Create new database instance
    pub async fn new(database_url: &str) -> LlmResult<Self> {
        // Parse connection options and ensure database is created if it doesn't exist
        let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        // Run migrations
        let migrations_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        if migrations_path.exists() {
            sqlx::migrate::Migrator::new(migrations_path)
                .await
                .map_err(|e| crate::error::LlmError::DatabaseError(e.to_string()))?
                .run(&pool)
                .await
                .map_err(|e| crate::error::LlmError::DatabaseError(e.to_string()))?;
        }

        Ok(Self {
            routing_rules: RoutingRulesDatabase::new(pool.clone()),
            predictions: PredictionsDatabase::new(pool.clone()),
            metrics: MetricsDatabase::new(pool.clone()),
            gepa: GepaDatabase::new(pool.clone()),
            pool,
        })
    }
}

impl Clone for LlmDatabase {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            routing_rules: self.routing_rules.clone(),
            predictions: self.predictions.clone(),
            metrics: self.metrics.clone(),
            gepa: self.gepa.clone(),
        }
    }
}
