//! Database operations for predictions and accuracy tracking

#[allow(unused_imports)]
use crate::error::LlmError as _;
use crate::error::LlmResult;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PredictionRecord {
    pub id: String,
    pub context_hash: String,
    pub predicted_tool: String,
    pub actual_tool: Option<String>,
    pub confidence: f64,
    pub prediction_data: Value,
    pub timestamp: DateTime<Utc>,
    pub correct: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct PredictionsDatabase {
    pool: SqlitePool,
}

impl PredictionsDatabase {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a new prediction
    pub async fn record_prediction(
        &self,
        context_hash: &str,
        predicted_tool: &str,
        confidence: f64,
        prediction_data: Value,
    ) -> LlmResult<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO predictions (id, context_hash, predicted_tool, confidence, prediction_data, timestamp) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(context_hash)
        .bind(predicted_tool)
        .bind(confidence)
        .bind(&prediction_data)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    /// Update prediction with actual result
    pub async fn update_prediction_result(
        &self,
        prediction_id: &str,
        actual_tool: &str,
    ) -> LlmResult<()> {
        let correct = self
            .get_prediction_tool(prediction_id)
            .await?
            .map_or(false, |predicted| predicted == actual_tool);

        sqlx::query("UPDATE predictions SET actual_tool = ?, correct = ? WHERE id = ?")
            .bind(actual_tool)
            .bind(correct)
            .bind(prediction_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Get prediction accuracy metrics
    pub async fn get_accuracy_metrics(&self, time_window_hours: i64) -> LlmResult<AccuracyMetrics> {
        let since = Utc::now() - chrono::Duration::hours(time_window_hours);

        #[derive(sqlx::FromRow)]
        struct MetricsRow {
            total_predictions: i64,
            correct_predictions: i64,
            avg_confidence: Option<f64>,
        }

        let metrics = sqlx::query_as::<_, MetricsRow>(
            "SELECT 
                COUNT(*) as total_predictions,
                SUM(CASE WHEN correct = true THEN 1 ELSE 0 END) as correct_predictions,
                AVG(confidence) as avg_confidence
            FROM predictions 
            WHERE timestamp > ? AND actual_tool IS NOT NULL",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        let accuracy = if metrics.total_predictions > 0 {
            metrics.correct_predictions as f64 / metrics.total_predictions as f64
        } else {
            0.0
        };

        Ok(AccuracyMetrics {
            accuracy,
            total_predictions: metrics.total_predictions,
            correct_predictions: metrics.correct_predictions,
            avg_confidence: metrics.avg_confidence.unwrap_or(0.0),
            time_window_hours,
        })
    }

    /// Get predictions by tool
    pub async fn get_predictions_by_tool(
        &self,
        tool_name: &str,
        limit: i64,
    ) -> LlmResult<Vec<PredictionRecord>> {
        let predictions = sqlx::query_as::<_, PredictionRecord>(
            "SELECT * FROM predictions WHERE predicted_tool = ? ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(tool_name)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(predictions)
    }

    /// Get recent predictions
    pub async fn get_recent_predictions(&self, limit: i64) -> LlmResult<Vec<PredictionRecord>> {
        let predictions = sqlx::query_as::<_, PredictionRecord>(
            "SELECT * FROM predictions ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(predictions)
    }

    async fn get_prediction_tool(&self, prediction_id: &str) -> LlmResult<Option<String>> {
        #[derive(sqlx::FromRow)]
        struct ToolRow {
            predicted_tool: String,
        }

        let result =
            sqlx::query_as::<_, ToolRow>("SELECT predicted_tool FROM predictions WHERE id = ?")
                .bind(prediction_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(result.map(|r| r.predicted_tool))
    }
}

#[derive(Debug, Clone)]
pub struct AccuracyMetrics {
    pub accuracy: f64,
    pub total_predictions: i64,
    pub correct_predictions: i64,
    pub avg_confidence: f64,
    pub time_window_hours: i64,
}

// Clone is derived automatically since SqlitePool implements Clone
