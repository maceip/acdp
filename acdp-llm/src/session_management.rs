//! Session and conversation tracking for LLM predictions
//!
//! This module integrates dspy-rs with MCP sessions to provide:
//! - Conversation context building from message history
//! - Per-session prediction tracking
//! - Session-level optimization via GEPA

use crate::dspy_signatures::ToolPrediction;
#[allow(unused_imports)]
use crate::error::LlmError as _;
use crate::error::LlmResult;
use crate::gepa_optimizer::GEPAOptimizer;
use crate::predictors::ToolPredictor;
use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use acdp_common::types::{MessageFlow, MessageId, ProxySession as _, SessionId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

/// Session-level tracking for LLM predictions
#[derive(Debug, Clone)]
pub struct SessionPredictionContext {
    pub session_id: SessionId,
    pub message_history: Vec<MessageFlow>,
    pub predictions: Vec<SessionPrediction>,
    pub total_predictions: u64,
    pub successful_predictions: u64,
    pub optimization_score: f32,
    pub last_updated: DateTime<Utc>,
}

impl SessionPredictionContext {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            message_history: Vec::new(),
            predictions: Vec::new(),
            total_predictions: 0,
            successful_predictions: 0,
            optimization_score: 0.0,
            last_updated: Utc::now(),
        }
    }

    /// Add a message to the conversation history
    pub fn add_message(&mut self, message: MessageFlow) {
        self.message_history.push(message);
        self.last_updated = Utc::now();
    }

    /// Add a prediction result
    pub fn add_prediction(&mut self, prediction: SessionPrediction) {
        self.total_predictions += 1;
        if prediction.was_accurate {
            self.successful_predictions += 1;
        }
        self.predictions.push(prediction);
        self.update_optimization_score();
        self.last_updated = Utc::now();
    }

    /// Calculate current prediction accuracy
    pub fn accuracy(&self) -> f32 {
        if self.total_predictions == 0 {
            return 0.0;
        }
        self.successful_predictions as f32 / self.total_predictions as f32
    }

    /// Update optimization score based on recent predictions
    fn update_optimization_score(&mut self) {
        let recent_window = 10;
        let recent_predictions: Vec<&SessionPrediction> =
            self.predictions.iter().rev().take(recent_window).collect();

        if recent_predictions.is_empty() {
            self.optimization_score = 0.0;
            return;
        }

        let accurate_count = recent_predictions.iter().filter(|p| p.was_accurate).count() as f32;

        let avg_confidence: f32 = recent_predictions.iter().map(|p| p.confidence).sum::<f32>()
            / recent_predictions.len() as f32;

        // Combined score: accuracy weighted with confidence
        self.optimization_score =
            (accurate_count / recent_predictions.len() as f32) * 0.7 + avg_confidence * 0.3;
    }

    /// Build MCP context string from message history for prediction
    pub fn build_mcp_context(&self) -> String {
        let mut context = String::new();

        context.push_str(&format!("Session: {}\n", self.session_id.0));
        context.push_str(&format!(
            "Accuracy: {:.2}%, Optimization Score: {:.2}\n",
            self.accuracy() * 100.0,
            self.optimization_score
        ));
        context.push_str("Recent Messages:\n");

        // Include last 5 messages for context
        for message in self.message_history.iter().rev().take(5).rev() {
            context.push_str(&format!(
                "  - Method: {}, Status: {:?}\n",
                message.client_request.method, message.status
            ));
        }

        context
    }
}

/// A prediction made for a specific message in a session
#[derive(Debug, Clone)]
pub struct SessionPrediction {
    pub message_id: MessageId,
    pub prediction: ToolPrediction,
    pub actual_tool: Option<String>,
    pub was_accurate: bool,
    pub confidence: f32,
    pub latency_ms: u64,
    pub timestamp: DateTime<Utc>,
    pub prediction_record_id: Option<String>,
}

impl SessionPrediction {
    pub fn new(
        message_id: MessageId,
        prediction: ToolPrediction,
        prediction_record_id: Option<String>,
        latency_ms: u64,
    ) -> Self {
        let confidence = prediction.confidence;
        Self {
            message_id,
            prediction,
            actual_tool: None,
            was_accurate: false,
            confidence,
            latency_ms,
            timestamp: Utc::now(),
            prediction_record_id,
        }
    }

    /// Mark the prediction with the actual tool used
    pub fn mark_actual(&mut self, actual_tool: String) {
        self.was_accurate = self.prediction.tool_name == actual_tool;
        self.actual_tool = Some(actual_tool);
    }
}

/// Session manager that tracks all active sessions and their predictions
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, SessionPredictionContext>>>,
    predictor: Arc<ToolPredictor>,
    gepa_optimizer: Option<Arc<GEPAOptimizer>>,
}

impl SessionManager {
    pub fn new(predictor: Arc<ToolPredictor>, gepa_optimizer: Option<Arc<GEPAOptimizer>>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            predictor,
            gepa_optimizer,
        }
    }

    /// Get or create a session context
    pub async fn get_or_create_session(&self, session_id: SessionId) -> SessionPredictionContext {
        let mut sessions = self.sessions.write().await;
        sessions
            .entry(session_id.clone())
            .or_insert_with(|| SessionPredictionContext::new(session_id.clone()))
            .clone()
    }

    /// Update a session context
    pub async fn update_session(&self, context: SessionPredictionContext) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(context.session_id.clone(), context);
    }

    /// Add a message to a session's history
    pub async fn add_message(&self, session_id: SessionId, message: MessageFlow) {
        let mut sessions = self.sessions.write().await;
        if let Some(context) = sessions.get_mut(&session_id) {
            context.add_message(message);
        }
    }

    /// Make a prediction for a session
    pub async fn predict_for_session(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        method: &str,
    ) -> LlmResult<SessionPrediction> {
        let start = std::time::Instant::now();

        // Get session context
        let context = self.get_or_create_session(session_id.clone()).await;

        // Build MCP context from session history
        let mcp_context = context.build_mcp_context();
        let mcp_context_with_method = format!("{}\nCurrent Method: {}\n", mcp_context, method);

        // Make prediction (recoding stored automatically)
        let prediction_outcome = self
            .predictor
            .predict_tool(&mcp_context_with_method)
            .await?;
        let tool_prediction = prediction_outcome.prediction.clone();

        let latency_ms = start.elapsed().as_millis() as u64;

        let session_prediction = SessionPrediction::new(
            message_id,
            tool_prediction,
            Some(prediction_outcome.record_id.clone()),
            latency_ms,
        );

        let mut updated_context = context;
        updated_context.add_prediction(session_prediction.clone());
        self.update_session(updated_context).await;

        Ok(session_prediction)
    }

    /// Record actual tool usage and update session
    pub async fn record_actual_tool(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        actual_tool: String,
    ) -> LlmResult<()> {
        let mut record_id = None;
        let mut predicted_tool_name = None;
        {
            let mut sessions = self.sessions.write().await;

            if let Some(context) = sessions.get_mut(&session_id) {
                if let Some(prediction) = context
                    .predictions
                    .iter_mut()
                    .find(|p| p.message_id == message_id)
                {
                    predicted_tool_name = Some(prediction.prediction.tool_name.clone());
                    prediction.mark_actual(actual_tool.clone());
                    record_id = prediction.prediction_record_id.clone();
                    context.last_updated = Utc::now();
                }
            }
        }

        if let Some(record_id) = record_id {
            self.predictor
                .update_prediction_result(&record_id, &actual_tool)
                .await?;
        }

        if let (Some(tool_name), Some(optimizer)) = (&predicted_tool_name, &self.gepa_optimizer) {
            if let Err(e) = optimizer.maybe_optimize_tool(tool_name).await {
                warn!("GEPA optimization attempt failed for {}: {}", tool_name, e);
            }
        }

        Ok(())
    }

    /// Record an externally generated prediction for a session.
    pub async fn record_prediction_for_session(
        &self,
        session_id: SessionId,
        prediction: SessionPrediction,
    ) {
        let mut sessions = self.sessions.write().await;
        let context = sessions
            .entry(session_id.clone())
            .or_insert_with(|| SessionPredictionContext::new(session_id.clone()));
        context.add_prediction(prediction);
    }

    /// Get session statistics
    pub async fn get_session_stats(&self, session_id: &SessionId) -> Option<SessionStats> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|context| SessionStats {
            session_id: context.session_id.clone(),
            total_predictions: context.total_predictions,
            successful_predictions: context.successful_predictions,
            accuracy: context.accuracy(),
            optimization_score: context.optimization_score,
            message_count: context.message_history.len() as u64,
            last_updated: context.last_updated,
        })
    }

    /// Get all session IDs
    pub async fn list_sessions(&self) -> Vec<SessionId> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// Clear old inactive sessions
    pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) {
        let mut sessions = self.sessions.write().await;
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);

        sessions.retain(|_id, context| context.last_updated > cutoff);
    }
}

/// Statistics for a session
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub session_id: SessionId,
    pub total_predictions: u64,
    pub successful_predictions: u64,
    pub accuracy: f32,
    pub optimization_score: f32,
    pub message_count: u64,
    pub last_updated: DateTime<Utc>,
}
