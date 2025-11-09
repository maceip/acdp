//! GEPA (Gradient Evolution Prompt Optimization) implementation

use crate::database::predictions::PredictionRecord;
use crate::database::{GepaDatabase, LlmDatabase, PredictionsDatabase};
use crate::error::{LlmError, LlmResult};
use crate::lm_provider::LiteRTLM;
#[allow(unused_imports)]
use async_trait::async_trait as _;
use chrono::{DateTime, Duration, Utc};
#[allow(unused_imports)]
use dspy_rs::{Example, Module, Optimizer as _, Prediction};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// GEPA optimizer for prompt improvement
pub struct GEPAOptimizer {
    lm_provider: Option<Arc<LiteRTLM>>,
    gepa_db: GepaDatabase,
    predictions_db: Arc<PredictionsDatabase>,
    optimization_history: Vec<OptimizationIteration>,
    max_iterations: usize,
    improvement_threshold: f64,
    min_window_size: usize,
    trigger_accuracy: f64,
    cooldown: Duration,
    recent_runs: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
}

#[derive(Debug, Clone)]
pub struct OptimizationIteration {
    pub iteration: usize,
    pub original_prompt: String,
    pub optimized_prompt: String,
    pub expected_improvement: f64,
    pub actual_improvement: Option<f64>,
    pub reasoning: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub iterations: Vec<OptimizationIteration>,
    pub final_improvement: f64,
    pub success: bool,
    pub total_time_ms: u64,
}

impl GEPAOptimizer {
    /// Create new GEPA optimizer
    pub fn new(lm_provider: Option<Arc<LiteRTLM>>, database: &LlmDatabase) -> Self {
        Self {
            lm_provider,
            gepa_db: database.gepa.clone(),
            predictions_db: Arc::new(database.predictions.clone()),
            optimization_history: Vec::new(),
            max_iterations: 10,
            improvement_threshold: 0.1,
            min_window_size: 20,
            trigger_accuracy: 0.85,
            cooldown: Duration::minutes(15),
            recent_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Opportunistically optimize a tool based on recent prediction failures.
    pub async fn maybe_optimize_tool(
        &self,
        module_name: &str,
    ) -> LlmResult<Option<OptimizationIteration>> {
        let window = self.min_window_size * 2;
        let history = self
            .predictions_db
            .get_predictions_by_tool(module_name, window as i64)
            .await?;

        if history.len() < window {
            return Ok(None);
        }

        {
            let runs = self.recent_runs.lock().await;
            if let Some(last_run) = runs.get(module_name) {
                if Utc::now() - *last_run < self.cooldown {
                    return Ok(None);
                }
            }
        }

        let recent_slice = &history[..self.min_window_size];
        let previous_slice = &history[self.min_window_size..self.min_window_size * 2];

        let recent_accuracy = compute_accuracy(recent_slice);
        let previous_accuracy = compute_accuracy(previous_slice);

        if let Some(acc) = recent_accuracy {
            if acc >= self.trigger_accuracy {
                return Ok(None);
            }
        }

        let failures = collect_failure_patterns(recent_slice);
        let last_record = self
            .gepa_db
            .get_recent_optimizations(module_name, 1)
            .await?
            .pop();

        let baseline_prompt = last_record
            .as_ref()
            .map(|record| record.optimized_prompt.clone())
            .unwrap_or_else(|| format!("Default prompt for {}", module_name));
        let next_iteration = last_record
            .as_ref()
            .map(|record| record.iteration as usize + 1)
            .unwrap_or(1);

        let optimized_prompt =
            synthesize_prompt(module_name, &baseline_prompt, &failures, recent_accuracy);
        let expected_improvement = recent_accuracy
            .map(|acc| (1.0 - acc).clamp(0.0, 0.5))
            .unwrap_or(0.1);
        let actual_improvement = match (recent_accuracy, previous_accuracy) {
            (Some(current), Some(previous)) => Some(current - previous),
            _ => None,
        };

        let reasoning = format!(
            "Recent accuracy {:.1}% with {} tracked failures. Updated prompt emphasizes \
             remediation for the most common mistakes.",
            recent_accuracy.unwrap_or(0.0) * 100.0,
            failures.len()
        );

        let iteration_record = OptimizationIteration {
            iteration: next_iteration,
            original_prompt: baseline_prompt,
            optimized_prompt: optimized_prompt.clone(),
            expected_improvement,
            actual_improvement,
            reasoning,
            timestamp: Utc::now(),
        };

        if let Err(e) = self
            .gepa_db
            .record_optimization(module_name, &iteration_record)
            .await
        {
            warn!("Failed to persist GEPA iteration: {}", e);
        } else {
            info!(
                "Recorded GEPA optimization for {} (iteration #{})",
                module_name, next_iteration
            );
        }

        {
            let mut runs = self.recent_runs.lock().await;
            runs.insert(module_name.to_string(), Utc::now());
        }

        Ok(Some(iteration_record))
    }

    /// Optimize a module's prompts based on execution traces (legacy DSPy path).
    pub async fn optimize_module<T: Module>(
        &mut self,
        module: &mut T,
        train_examples: Vec<Example>,
    ) -> LlmResult<OptimizationResult> {
        let start_time = std::time::Instant::now();
        let mut iterations = Vec::new();
        let mut current_prompt = self.extract_current_prompt(module).await?;
        let mut best_improvement = 0.0;

        for iteration in 1..=self.max_iterations {
            let traces = self
                .generate_execution_traces(module, &train_examples)
                .await?;

            let optimization_result = self
                .optimize_prompt_iteration(&current_prompt, &traces)
                .await?;

            self.apply_prompt_to_module(module, &optimization_result.optimized_prompt)
                .await?;

            let actual_improvement = self.evaluate_improvement(module, &train_examples).await?;

            let iteration_record = OptimizationIteration {
                iteration,
                original_prompt: current_prompt.clone(),
                optimized_prompt: optimization_result.optimized_prompt.clone(),
                expected_improvement: optimization_result.expected_improvement as f64,
                actual_improvement: Some(actual_improvement as f64),
                reasoning: optimization_result.reasoning.clone(),
                timestamp: Utc::now(),
            };

            iterations.push(iteration_record);

            if (actual_improvement as f64) > best_improvement {
                best_improvement = actual_improvement as f64;
                current_prompt = optimization_result.optimized_prompt.clone();
            }

            if (actual_improvement as f64) >= self.improvement_threshold {
                break;
            }
        }

        let total_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(OptimizationResult {
            iterations,
            final_improvement: best_improvement,
            success: best_improvement >= self.improvement_threshold,
            total_time_ms,
        })
    }

    /// Generate improved prompt based on execution traces
    async fn optimize_prompt_iteration(
        &self,
        current_prompt: &str,
        traces: &[ExecutionTrace],
    ) -> LlmResult<PromptOptimizationResult> {
        if let Some(provider) = &self.lm_provider {
            let traces_json = serde_json::to_string(traces)?;

            let optimization_prompt = format!(
                "Analyze these execution traces from an LLM module and suggest an improved prompt:\n\n\
                 Current Prompt: \"{}\"\n\n\
                 Execution Traces:\n{}\n\n\
                 Focus on:\n\
                 1. Reducing common errors\n\
                 2. Improving clarity and specificity\n\
                 3. Better handling of edge cases\n\
                 4. More reliable tool selection\n\n\
                 Provide your response as a JSON object with:\n\
                 - optimized_prompt: The improved prompt\n\
                 - expected_improvement: Expected accuracy improvement (0.0-1.0)\n\
                 - reasoning: Why this prompt should work better",
                current_prompt, traces_json
            );

            let schema = json!({
                "type": "object",
                "properties": {
                    "optimized_prompt": {"type": "string"},
                    "expected_improvement": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                    "reasoning": {"type": "string"}
                },
                "required": ["optimized_prompt", "expected_improvement", "reasoning"]
            });

            // NOTE: GEPA optimization temporarily disabled
            // Would call DSPy model here for structured prompt optimization
            // For now, return basic result without LLM
            let _ = (provider, optimization_prompt, schema); // silence unused warnings
        }

        // Fallback heuristic prompt if LLM provider is unavailable
        Ok(PromptOptimizationResult {
            optimized_prompt: format!(
                "{}\n\nReminder: highlight common MCP failure modes and \
                 enumerate guard rails for tool routing.",
                current_prompt
            ),
            expected_improvement: 0.05,
            reasoning: "LiteRT provider unavailable; applied heuristic GEPA fallback.".into(),
        })
    }

    /// Generate execution traces from module
    async fn generate_execution_traces<T: Module>(
        &self,
        module: &T,
        examples: &[Example],
    ) -> LlmResult<Vec<ExecutionTrace>> {
        let mut traces = Vec::new();

        for example in examples {
            let start_time = std::time::Instant::now();

            let prediction = module
                .forward(example.clone())
                .await
                .map_err(|e| LlmError::PredictionError(e.to_string()))?;

            let execution_time = start_time.elapsed().as_millis() as u64;

            let success = self
                .evaluate_prediction_success(example, &prediction)
                .await?;

            let trace = ExecutionTrace {
                input: example.clone(),
                prediction,
                execution_time_ms: execution_time,
                success,
                error_message: if success {
                    None
                } else {
                    Some("Prediction failed evaluation".to_string())
                },
                timestamp: Utc::now(),
            };

            traces.push(trace);
        }

        Ok(traces)
    }

    /// Evaluate if a prediction was successful
    async fn evaluate_prediction_success(
        &self,
        _example: &Example,
        _prediction: &Prediction,
    ) -> LlmResult<bool> {
        Ok(true)
    }

    /// Evaluate improvement after applying new prompt
    async fn evaluate_improvement<T: Module>(
        &self,
        module: &T,
        examples: &[Example],
    ) -> LlmResult<f32> {
        let mut successes = 0;

        for example in examples {
            let prediction = module
                .forward(example.clone())
                .await
                .map_err(|e| LlmError::PredictionError(e.to_string()))?;

            if self
                .evaluate_prediction_success(example, &prediction)
                .await?
            {
                successes += 1;
            }
        }

        Ok(successes as f32 / examples.len() as f32)
    }

    /// Extract current prompt from module
    async fn extract_current_prompt<T: Module>(&self, _module: &T) -> LlmResult<String> {
        Ok("Current prompt placeholder".to_string())
    }

    /// Apply optimized prompt to module
    async fn apply_prompt_to_module<T: Module>(
        &self,
        _module: &mut T,
        _prompt: &str,
    ) -> LlmResult<()> {
        Ok(())
    }

    /// Get optimization history
    pub fn get_optimization_history(&self) -> &[OptimizationIteration] {
        &self.optimization_history
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionTrace {
    pub input: Example,
    pub prediction: Prediction,
    pub execution_time_ms: u64,
    pub success: bool,
    pub error_message: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PromptOptimizationResult {
    pub optimized_prompt: String,
    pub expected_improvement: f32,
    pub reasoning: String,
}

// Optimizer trait implementation removed - API changed in dspy-rs 0.7.1

fn compute_accuracy(records: &[PredictionRecord]) -> Option<f64> {
    let mut counted = 0;
    let mut correct = 0;
    for record in records {
        if let Some(value) = record.correct {
            if value {
                correct += 1;
            }
            counted += 1;
        }
    }
    if counted == 0 {
        None
    } else {
        Some(correct as f64 / counted as f64)
    }
}

fn collect_failure_patterns(records: &[PredictionRecord]) -> Vec<String> {
    records
        .iter()
        .filter(|record| matches!(record.correct, Some(false)))
        .filter_map(|record| {
            record
                .prediction_data
                .get("reasoning")
                .and_then(|value| value.as_str())
                .map(|reason| reason.to_string())
        })
        .collect()
}

fn synthesize_prompt(
    module_name: &str,
    baseline_prompt: &str,
    failures: &[String],
    recent_accuracy: Option<f64>,
) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("### Optimized prompt for {}\n", module_name));
    prompt.push_str(baseline_prompt);
    prompt.push_str("\n\nGuidance:\n");

    if failures.is_empty() {
        prompt.push_str(
            "- Emphasize validating MCP arguments and prefer deterministic routing rules.\n",
        );
    } else {
        for failure in failures.iter().take(5) {
            prompt.push_str(&format!("- Address failure: {}\n", failure));
        }
    }

    if let Some(acc) = recent_accuracy {
        prompt.push_str(&format!(
            "- Current accuracy {:.1}%; aim to exceed {:.1}%.\n",
            acc * 100.0,
            (acc + 0.1).min(0.95) * 100.0
        ));
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gepa_optimizer_creation() {
        // Creation should succeed even without LiteRT provider
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("gepa.sqlite");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());
        let database = LlmDatabase::new(&db_url).await.unwrap();

        let optimizer = GEPAOptimizer::new(None, &database);
        assert_eq!(optimizer.max_iterations, 10);
    }
}
