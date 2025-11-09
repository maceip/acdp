//! Database operations for GEPA optimization tracking

use crate::error::LlmResult;
use crate::gepa_optimizer::OptimizationIteration;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GepaOptimizationRecord {
    pub id: String,
    pub module_name: String,
    pub iteration: i64,
    pub original_prompt: String,
    pub optimized_prompt: String,
    pub expected_improvement: f64,
    pub actual_improvement: Option<f64>,
    pub reasoning: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GepaDatabase {
    pool: SqlitePool,
}

impl GepaDatabase {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a GEPA optimization iteration
    pub async fn record_optimization(
        &self,
        module_name: &str,
        iteration: &OptimizationIteration,
    ) -> LlmResult<String> {
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO gepa_optimizations
            (id, module_name, iteration, original_prompt, optimized_prompt,
             expected_improvement, actual_improvement, reasoning, timestamp)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(module_name)
        .bind(iteration.iteration as i64)
        .bind(&iteration.original_prompt)
        .bind(&iteration.optimized_prompt)
        .bind(iteration.expected_improvement)
        .bind(iteration.actual_improvement)
        .bind(&iteration.reasoning)
        .bind(iteration.timestamp)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    /// Get recent optimizations for a module
    pub async fn get_recent_optimizations(
        &self,
        module_name: &str,
        limit: i64,
    ) -> LlmResult<Vec<GepaOptimizationRecord>> {
        let records = sqlx::query_as::<_, GepaOptimizationRecord>(
            "SELECT * FROM gepa_optimizations
            WHERE module_name = ?
            ORDER BY timestamp DESC
            LIMIT ?",
        )
        .bind(module_name)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(records)
    }

    /// Get all optimizations for a module
    pub async fn get_all_optimizations(
        &self,
        module_name: &str,
    ) -> LlmResult<Vec<GepaOptimizationRecord>> {
        let records = sqlx::query_as::<_, GepaOptimizationRecord>(
            "SELECT * FROM gepa_optimizations
            WHERE module_name = ?
            ORDER BY iteration ASC",
        )
        .bind(module_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(records)
    }

    /// Get total improvement for a module (latest iteration's actual improvement)
    pub async fn get_total_improvement(&self, module_name: &str) -> LlmResult<Option<f64>> {
        #[derive(sqlx::FromRow)]
        struct ImprovementRow {
            actual_improvement: Option<f64>,
        }

        let result = sqlx::query_as::<_, ImprovementRow>(
            "SELECT actual_improvement
            FROM gepa_optimizations
            WHERE module_name = ? AND actual_improvement IS NOT NULL
            ORDER BY iteration DESC
            LIMIT 1",
        )
        .bind(module_name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.and_then(|r| r.actual_improvement))
    }

    /// Get average improvement across all modules
    pub async fn get_average_improvement(&self) -> LlmResult<Option<f64>> {
        #[derive(sqlx::FromRow)]
        struct AvgRow {
            avg_improvement: Option<f64>,
        }

        let result = sqlx::query_as::<_, AvgRow>(
            "SELECT AVG(actual_improvement) as avg_improvement
            FROM gepa_optimizations
            WHERE actual_improvement IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.avg_improvement)
    }
}
