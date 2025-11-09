//! Conversation context building from MessageFlow history
//!
//! This module provides utilities to build rich conversation context
//! from MCP message flows for use in LLM predictions.

// DateTime and Utc kept for potential future use - not currently used
use acdp_common::types::{MessageFlow, MessageStatus, ProxySession};

/// Builder for creating conversation context from message history
pub struct ConversationContextBuilder {
    messages: Vec<MessageFlow>,
    session: Option<ProxySession>,
    max_messages: usize,
    include_timing: bool,
    include_parameters: bool,
    include_predictions: bool,
}

impl ConversationContextBuilder {
    /// Create a new context builder
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            session: None,
            max_messages: 10,
            include_timing: true,
            include_parameters: false,
            include_predictions: true,
        }
    }

    /// Add messages to the context
    pub fn with_messages(mut self, messages: Vec<MessageFlow>) -> Self {
        self.messages = messages;
        self
    }

    /// Add session information
    pub fn with_session(mut self, session: ProxySession) -> Self {
        self.session = Some(session);
        self
    }

    /// Set maximum number of messages to include
    pub fn max_messages(mut self, max: usize) -> Self {
        self.max_messages = max;
        self
    }

    /// Include timing information
    pub fn include_timing(mut self, include: bool) -> Self {
        self.include_timing = include;
        self
    }

    /// Include request parameters
    pub fn include_parameters(mut self, include: bool) -> Self {
        self.include_parameters = include;
        self
    }

    /// Include previous predictions
    pub fn include_predictions(mut self, include: bool) -> Self {
        self.include_predictions = include;
        self
    }

    /// Build the conversation context string
    pub fn build(self) -> String {
        let mut context = String::new();

        // Add session information if available
        if let Some(session) = &self.session {
            context.push_str("=== Session Context ===\n");
            context.push_str(&format!("Session ID: {}\n", session.id.0));
            context.push_str(&format!("Request Count: {}\n", session.request_count));
            context.push_str(&format!("Status: {:?}\n", session.status));

            // TODO: LLM prediction metrics would be tracked separately
            // ProxySession doesn't have llm_predictions field - would need to add or track separately
            // if let Some(llm_metrics) = &session.llm_predictions {
            //     context.push_str(&format!(
            //         "Prediction Accuracy: {:.1}% ({}/{})\n",
            //         llm_metrics.accuracy * 100.0,
            //         llm_metrics.successful_predictions,
            //         llm_metrics.total_predictions
            //     ));
            //     context.push_str(&format!(
            //         "Optimization Score: {:.2}\n",
            //         llm_metrics.optimization_score
            //     ));
            // }

            context.push_str("\n");
        }

        // Add message history
        context.push_str("=== Conversation History ===\n");

        let recent_messages: Vec<&MessageFlow> = self
            .messages
            .iter()
            .rev()
            .take(self.max_messages)
            .rev()
            .collect();

        if recent_messages.is_empty() {
            context.push_str("(No previous messages)\n");
        } else {
            for (idx, message) in recent_messages.iter().enumerate() {
                context.push_str(&format!("\n[Message {}]\n", idx + 1));
                context.push_str(&format!("Method: {}\n", message.client_request.method));
                context.push_str(&format!("Status: {:?}\n", message.status));

                if self.include_timing {
                    let duration = if let Some(responded_at) = message.timing.responded_at {
                        responded_at
                            .signed_duration_since(message.timing.received_at)
                            .num_milliseconds()
                    } else {
                        0
                    };
                    context.push_str(&format!("Duration: {}ms\n", duration));
                }

                if self.include_parameters {
                    if let Some(params) = &message.client_request.params {
                        context.push_str(&format!(
                            "Parameters: {}\n",
                            serde_json::to_string_pretty(params)
                                .unwrap_or_else(|_| "{}".to_string())
                        ));
                    }
                }

                // TODO: LLM predictions would be tracked separately
                // MessageFlow doesn't have llm_prediction field - would need to add or track separately
                // if self.include_predictions {
                //     if let Some(prediction) = &message.llm_prediction {
                //         context.push_str(&format!(
                //             "Predicted Tool: {} (confidence: {:.2})\n",
                //             prediction.predicted_tool,
                //             prediction.confidence
                //         ));
                //         if let Some(actual) = &prediction.actual_tool {
                //             let accuracy_marker = if prediction.was_accurate { "✓" } else { "✗" };
                //             context.push_str(&format!(
                //                 "Actual Tool: {} {}\n",
                //                 actual,
                //                 accuracy_marker
                //             ));
                //         }
                //     }
                // }

                // Add transformation information
                if !message.transformations.is_empty() {
                    context.push_str("Transformations Applied: ");
                    let transform_names: Vec<String> = message
                        .transformations
                        .iter()
                        .map(|t| t.rule_name.clone())
                        .collect();
                    context.push_str(&transform_names.join(", "));
                    context.push_str("\n");
                }
            }
        }

        context.push_str("\n=== Current Request ===\n");
        context
    }
}

impl Default for ConversationContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract conversation patterns from message history
pub struct ConversationAnalyzer;

impl ConversationAnalyzer {
    /// Analyze message patterns to identify common sequences
    pub fn analyze_patterns(messages: &[MessageFlow]) -> ConversationPatterns {
        let mut patterns = ConversationPatterns::default();

        if messages.is_empty() {
            return patterns;
        }

        // Count method frequencies
        for message in messages {
            let method = &message.client_request.method;
            *patterns.method_frequency.entry(method.clone()).or_insert(0) += 1;
        }

        // Find most common method
        if let Some((method, _count)) = patterns
            .method_frequency
            .iter()
            .max_by_key(|(_, count)| *count)
        {
            patterns.most_common_method = Some(method.clone());
        }

        // Calculate average success rate
        let completed_count = messages
            .iter()
            .filter(|m| matches!(m.status, MessageStatus::Completed))
            .count();
        patterns.success_rate = if !messages.is_empty() {
            completed_count as f32 / messages.len() as f32
        } else {
            0.0
        };

        // Calculate average duration
        let mut total_duration_ms = 0u64;
        let mut duration_count = 0u64;
        for message in messages {
            if let Some(responded_at) = message.timing.responded_at {
                let duration = responded_at
                    .signed_duration_since(message.timing.received_at)
                    .num_milliseconds();
                if duration >= 0 {
                    total_duration_ms += duration as u64;
                    duration_count += 1;
                }
            }
        }
        patterns.average_duration_ms = if duration_count > 0 {
            total_duration_ms / duration_count
        } else {
            0
        };

        // Identify sequential patterns (method sequences)
        if messages.len() >= 2 {
            for window in messages.windows(2) {
                let sequence = format!(
                    "{} -> {}",
                    window[0].client_request.method, window[1].client_request.method
                );
                *patterns.sequence_patterns.entry(sequence).or_insert(0) += 1;
            }
        }

        patterns
    }

    /// Get prediction hints based on conversation patterns
    pub fn get_prediction_hints(patterns: &ConversationPatterns) -> Vec<String> {
        let mut hints = Vec::new();

        if let Some(method) = &patterns.most_common_method {
            hints.push(format!("Most frequently used method: {}", method));
        }

        if patterns.success_rate < 0.5 {
            hints.push("Low success rate - consider error handling".to_string());
        }

        if patterns.average_duration_ms > 5000 {
            hints.push("High average latency detected".to_string());
        }

        // Add sequence pattern hints
        if let Some((sequence, count)) = patterns
            .sequence_patterns
            .iter()
            .max_by_key(|(_, count)| *count)
        {
            if *count >= 2 {
                hints.push(format!(
                    "Common sequence: {} (observed {} times)",
                    sequence, count
                ));
            }
        }

        hints
    }
}

/// Patterns extracted from conversation analysis
#[derive(Debug, Clone, Default)]
pub struct ConversationPatterns {
    pub method_frequency: std::collections::HashMap<String, usize>,
    pub most_common_method: Option<String>,
    pub success_rate: f32,
    pub average_duration_ms: u64,
    pub sequence_patterns: std::collections::HashMap<String, usize>,
}
