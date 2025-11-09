//! Transform interceptor for rule-based message modification

use futures::future::{BoxFuture, FutureExt};
use acdp_core::interceptor::{
    InterceptionResult, InterceptorStats, MessageContext, MessageInterceptor,
};
use acdp_core::messages::JsonRpcMessage;
use acdp_core::McpResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// A rule for transforming JSON-RPC messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformRule {
    /// Name of this rule
    pub name: String,
    /// Method pattern to match (e.g., "tools/call", "*" for all)
    pub method_pattern: String,
    /// JSON path to modify (e.g., "params.arguments.verbose")
    pub path: String,
    /// Transformation operation
    pub operation: TransformOperation,
}

/// Operations that can be performed on message fields
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformOperation {
    /// Set a field to a specific value
    Set { value: Value },
    /// Add a field if it doesn't exist
    AddIfMissing { value: Value },
    /// Remove a field
    Remove,
    /// Rename a field
    Rename { new_name: String },
    /// Apply a function (limited set for safety)
    Function { name: String, args: Vec<Value> },
}

impl TransformRule {
    /// Check if this rule matches the given message
    fn matches(&self, context: &MessageContext) -> bool {
        if let Some(method) = context.method() {
            self.method_pattern == "*" || self.method_pattern == method
        } else {
            false
        }
    }

    /// Apply this rule to a message
    fn apply(&self, message: &JsonRpcMessage) -> Result<JsonRpcMessage, String> {
        let mut modified = message.clone();

        match &mut modified {
            JsonRpcMessage::Request(ref mut req) => {
                if let Some(ref mut params) = req.params {
                    self.apply_to_value(params)?;
                }
            }
            JsonRpcMessage::Response(ref mut resp) => {
                if let Some(ref mut result) = resp.result {
                    self.apply_to_value(result)?;
                }
            }
            JsonRpcMessage::Notification(ref mut notif) => {
                if let Some(ref mut params) = notif.params {
                    self.apply_to_value(params)?;
                }
            }
        }

        Ok(modified)
    }

    /// Apply transformation to a JSON value using path
    fn apply_to_value(&self, value: &mut Value) -> Result<(), String> {
        let path_parts: Vec<&str> = self.path.split('.').collect();

        match &self.operation {
            TransformOperation::Set { value: new_value } => {
                self.set_at_path(value, &path_parts, new_value.clone())?;
            }
            TransformOperation::AddIfMissing { value: new_value } => {
                if self.get_at_path(value, &path_parts).is_none() {
                    self.set_at_path(value, &path_parts, new_value.clone())?;
                }
            }
            TransformOperation::Remove => {
                self.remove_at_path(value, &path_parts)?;
            }
            TransformOperation::Rename { new_name } => {
                if let Some(last) = path_parts.last() {
                    let parent_path = &path_parts[..path_parts.len() - 1];
                    if let Some(parent) = self.get_at_path_mut(value, parent_path) {
                        if let Some(obj) = parent.as_object_mut() {
                            if let Some(val) = obj.remove(*last) {
                                obj.insert(new_name.clone(), val);
                            }
                        }
                    }
                }
            }
            TransformOperation::Function { name, args } => {
                self.apply_function(value, &path_parts, name, args)?;
            }
        }

        Ok(())
    }

    fn get_at_path<'a>(&self, value: &'a Value, path: &[&str]) -> Option<&'a Value> {
        let mut current = value;
        for part in path {
            current = current.get(part)?;
        }
        Some(current)
    }

    fn get_at_path_mut<'a>(&self, value: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
        let mut current = value;
        for part in path {
            current = current.get_mut(part)?;
        }
        Some(current)
    }

    fn set_at_path(
        &self,
        value: &mut Value,
        path: &[&str],
        new_value: Value,
    ) -> Result<(), String> {
        if path.is_empty() {
            return Err("Empty path".to_string());
        }

        if path.len() == 1 {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(path[0].to_string(), new_value);
                return Ok(());
            }
            return Err("Cannot set field on non-object".to_string());
        }

        let parent_path = &path[..path.len() - 1];
        let field_name = path[path.len() - 1];

        if let Some(parent) = self.get_at_path_mut(value, parent_path) {
            if let Some(obj) = parent.as_object_mut() {
                obj.insert(field_name.to_string(), new_value);
                return Ok(());
            }
        }

        Err("Path not found".to_string())
    }

    fn remove_at_path(&self, value: &mut Value, path: &[&str]) -> Result<(), String> {
        if path.is_empty() {
            return Err("Empty path".to_string());
        }

        if path.len() == 1 {
            if let Some(obj) = value.as_object_mut() {
                obj.remove(path[0]);
                return Ok(());
            }
            return Err("Cannot remove field from non-object".to_string());
        }

        let parent_path = &path[..path.len() - 1];
        let field_name = path[path.len() - 1];

        if let Some(parent) = self.get_at_path_mut(value, parent_path) {
            if let Some(obj) = parent.as_object_mut() {
                obj.remove(field_name);
                return Ok(());
            }
        }

        Ok(())
    }

    fn apply_function(
        &self,
        value: &mut Value,
        path: &[&str],
        func_name: &str,
        _args: &[Value],
    ) -> Result<(), String> {
        if let Some(target) = self.get_at_path_mut(value, path) {
            match func_name {
                "uppercase" => {
                    if let Some(s) = target.as_str() {
                        *target = Value::String(s.to_uppercase());
                    }
                }
                "lowercase" => {
                    if let Some(s) = target.as_str() {
                        *target = Value::String(s.to_lowercase());
                    }
                }
                "increment" => {
                    if let Some(n) = target.as_i64() {
                        *target = Value::Number((n + 1).into());
                    }
                }
                _ => return Err(format!("Unknown function: {}", func_name)),
            }
        }
        Ok(())
    }
}

/// Interceptor that applies transformation rules to messages
pub struct TransformInterceptor {
    name: String,
    stats: Arc<RwLock<InterceptorStats>>,
    rules: Arc<RwLock<Vec<TransformRule>>>,
}

impl TransformInterceptor {
    /// Create a new transform interceptor
    pub fn new() -> Self {
        Self {
            name: "TransformInterceptor".to_string(),
            stats: Arc::new(RwLock::new(InterceptorStats::default())),
            rules: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a transformation rule
    pub async fn add_rule(&self, rule: TransformRule) {
        let mut rules = self.rules.write().await;
        rules.push(rule);
    }

    /// Remove a rule by name
    pub async fn remove_rule(&self, name: &str) -> bool {
        let mut rules = self.rules.write().await;
        let initial_len = rules.len();
        rules.retain(|r| r.name != name);
        rules.len() != initial_len
    }

    /// List all rules
    pub async fn list_rules(&self) -> Vec<String> {
        let rules = self.rules.read().await;
        rules.iter().map(|r| r.name.clone()).collect()
    }
}

impl Default for TransformInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageInterceptor for TransformInterceptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> u32 {
        // Run after validation and rate limiting
        40
    }

    fn should_intercept<'a>(&'a self, context: &'a MessageContext) -> BoxFuture<'a, bool> {
        async move {
            let rules = self.rules.read().await;
            rules.iter().any(|rule| rule.matches(context))
        }
        .boxed()
    }

    fn intercept<'a>(
        &'a self,
        context: MessageContext,
    ) -> BoxFuture<'a, McpResult<InterceptionResult>> {
        async move {
            let start = std::time::Instant::now();

            let rules = self.rules.read().await;
            let mut current_message = context.message.clone();
            let mut was_modified = false;
            let mut applied_rules = Vec::new();

            for rule in rules.iter() {
                if rule.matches(&context) {
                    match rule.apply(&current_message) {
                        Ok(modified) => {
                            current_message = modified;
                            was_modified = true;
                            applied_rules.push(rule.name.clone());
                            debug!("[{}] Applied rule: {}", self.name, rule.name);
                        }
                        Err(e) => {
                            warn!("[{}] Rule '{}' failed: {}", self.name, rule.name, e);
                        }
                    }
                }
            }
            drop(rules);

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_intercepted += 1;
            if was_modified {
                stats.total_modified += 1;
            }
            stats.last_processed = Some(chrono::Utc::now());

            let elapsed = start.elapsed().as_millis() as f64;
            stats.avg_processing_time_ms =
                (stats.avg_processing_time_ms * (stats.total_intercepted - 1) as f64 + elapsed)
                    / stats.total_intercepted as f64;

            if was_modified {
                Ok(InterceptionResult::modified(
                    current_message,
                    format!("Applied rules: {}", applied_rules.join(", ")),
                    1.0,
                ))
            } else {
                Ok(InterceptionResult::pass_through(current_message))
            }
        }
        .boxed()
    }

    fn get_stats<'a>(&'a self) -> BoxFuture<'a, InterceptorStats> {
        async move { self.stats.read().await.clone() }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp_core::interceptor::MessageDirection;
    use acdp_core::messages::{JsonRpcRequest, RequestId};
    use serde_json::json;

    #[tokio::test]
    async fn test_transform_set_field() {
        let interceptor = TransformInterceptor::new();

        // Add rule to set verbose=true
        interceptor
            .add_rule(TransformRule {
                name: "add-verbose".to_string(),
                method_pattern: "tools/call".to_string(),
                path: "arguments.verbose".to_string(),
                operation: TransformOperation::Set { value: json!(true) },
            })
            .await;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "test_tool",
                "arguments": {}
            })),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(result.modified);
        if let JsonRpcMessage::Request(modified_req) = result.message {
            let params = modified_req.params.unwrap();
            assert_eq!(params["arguments"]["verbose"], json!(true));
        } else {
            panic!("Expected Request message");
        }
    }

    #[tokio::test]
    async fn test_transform_add_if_missing() {
        let interceptor = TransformInterceptor::new();

        interceptor
            .add_rule(TransformRule {
                name: "default-timeout".to_string(),
                method_pattern: "*".to_string(),
                path: "timeout".to_string(),
                operation: TransformOperation::AddIfMissing {
                    value: json!(30000),
                },
            })
            .await;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "test"
            })),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(result.modified);
        if let JsonRpcMessage::Request(modified_req) = result.message {
            let params = modified_req.params.unwrap();
            assert_eq!(params["timeout"], json!(30000));
        }
    }

    #[tokio::test]
    async fn test_transform_remove_field() {
        let interceptor = TransformInterceptor::new();

        interceptor
            .add_rule(TransformRule {
                name: "remove-debug".to_string(),
                method_pattern: "*".to_string(),
                path: "debug".to_string(),
                operation: TransformOperation::Remove,
            })
            .await;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            method: "test/method".to_string(),
            params: Some(json!({
                "debug": true,
                "data": "value"
            })),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(result.modified);
        if let JsonRpcMessage::Request(modified_req) = result.message {
            let params = modified_req.params.unwrap();
            assert!(params.get("debug").is_none());
            assert_eq!(params["data"], json!("value"));
        }
    }

    #[tokio::test]
    async fn test_transform_function_uppercase() {
        let interceptor = TransformInterceptor::new();

        interceptor
            .add_rule(TransformRule {
                name: "uppercase-name".to_string(),
                method_pattern: "*".to_string(),
                path: "name".to_string(),
                operation: TransformOperation::Function {
                    name: "uppercase".to_string(),
                    args: vec![],
                },
            })
            .await;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            method: "test/method".to_string(),
            params: Some(json!({
                "name": "hello"
            })),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(result.modified);
        if let JsonRpcMessage::Request(modified_req) = result.message {
            let params = modified_req.params.unwrap();
            assert_eq!(params["name"], json!("HELLO"));
        }
    }

    #[tokio::test]
    async fn test_transform_no_match() {
        let interceptor = TransformInterceptor::new();

        interceptor
            .add_rule(TransformRule {
                name: "specific-rule".to_string(),
                method_pattern: "tools/call".to_string(),
                path: "test".to_string(),
                operation: TransformOperation::Set { value: json!(true) },
            })
            .await;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            method: "other/method".to_string(),
            params: Some(json!({})),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(!result.modified);
    }
}
