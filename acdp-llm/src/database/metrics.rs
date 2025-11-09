//! Database operations for performance metrics

#[allow(unused_imports)]
use crate::error::LlmError as _;
use crate::error::LlmResult;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PerformanceMetric {
    pub id: String,
    pub metric_type: String,
    pub value: f64,
    pub tags: String, // JSON string
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MetricsDatabase {
    pool: SqlitePool,
}

impl MetricsDatabase {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a performance metric
    pub async fn record_metric(
        &self,
        metric_type: &str,
        value: f64,
        tags: &str,
    ) -> LlmResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO performance_metrics (id, metric_type, value, tags, timestamp) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(metric_type)
        .bind(value)
        .bind(tags)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    /// Get metrics by type and time range
    pub async fn get_metrics(
        &self,
        metric_type: &str,
        since: DateTime<Utc>,
    ) -> LlmResult<Vec<PerformanceMetric>> {
        let metrics = sqlx::query_as::<_, PerformanceMetric>(
            "SELECT * FROM performance_metrics WHERE metric_type = ? AND timestamp > ? ORDER BY timestamp DESC"
        )
        .bind(metric_type)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        Ok(metrics)
    }

    /// Get average metric value
    pub async fn get_average_metric(
        &self,
        metric_type: &str,
        time_window_hours: i64,
    ) -> LlmResult<Option<f64>> {
        let since = Utc::now() - chrono::Duration::hours(time_window_hours);

        #[derive(sqlx::FromRow)]
        struct AvgRow {
            avg_value: Option<f64>,
        }

        let result = sqlx::query_as::<_, AvgRow>(
            "SELECT AVG(value) as avg_value FROM performance_metrics WHERE metric_type = ? AND timestamp > ?"
        )
        .bind(metric_type)
        .bind(since)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.and_then(|r| r.avg_value))
    }

    /// Get metric trends
    pub async fn get_metric_trends(
        &self,
        metric_type: &str,
        time_window_hours: i64,
    ) -> LlmResult<MetricTrend> {
        let since = Utc::now() - chrono::Duration::hours(time_window_hours);

        #[derive(sqlx::FromRow)]
        struct TrendRow {
            avg_value: Option<f64>,
            min_value: Option<f64>,
            max_value: Option<f64>,
            sample_count: i64,
        }

        let trends = sqlx::query_as::<_, TrendRow>(
            "SELECT 
                AVG(value) as avg_value,
                MIN(value) as min_value,
                MAX(value) as max_value,
                COUNT(*) as sample_count
            FROM performance_metrics 
            WHERE metric_type = ? AND timestamp > ?",
        )
        .bind(metric_type)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        Ok(MetricTrend {
            metric_type: metric_type.to_string(),
            avg_value: trends.avg_value.unwrap_or(0.0),
            min_value: trends.min_value.unwrap_or(0.0),
            max_value: trends.max_value.unwrap_or(0.0),
            sample_count: trends.sample_count,
            time_window_hours,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MetricTrend {
    pub metric_type: String,
    pub avg_value: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub sample_count: i64,
    pub time_window_hours: i64,
}

// Clone is derived automatically since SqlitePool implements Clone
