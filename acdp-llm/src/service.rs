//! High-level LLM service facade used by both TUI and headless modes.
//!
//! This wraps the underlying `ModelManager` (and eventually the session/routing
//! stack) and exposes an async API for ensuring models are available, driving
//! streamed generations, and subscribing to service events.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{broadcast, mpsc};

use crate::config::AppConfig;
use crate::database::{GepaDatabase, LlmDatabase, PredictionsDatabase};
use crate::error::{LlmError, LlmResult};
use crate::gepa_optimizer::GEPAOptimizer;
use crate::lm_provider::{LiteRTBuilder, LiteRTLM};
use crate::model_management::{DownloadProgress, ModelManager, ModelStatus, ModelStatusUpdate};
use crate::predictors::{SemanticEngine, ToolPredictor};
use crate::session_management::SessionManager;
use crate::streaming::generate_streaming;
use serde_json::Value;
use tracing::warn;

/// Public service facade for LiteRT-backed LLM operations.
pub struct LlmService {
    model_manager: Arc<ModelManager>,
    database: Arc<LlmDatabase>,
    events_tx: broadcast::Sender<LlmEvent>,
    tool_predictor: Arc<ToolPredictor>,
    session_manager: Arc<SessionManager>,
}

impl LlmService {
    /// Build a new service instance from configuration.
    pub async fn new(config: AppConfig) -> LlmResult<Arc<Self>> {
        tracing::info!("[LLM_STARTUP] Creating ModelManager...");
        let manager_start = std::time::Instant::now();
        let manager = Arc::new(ModelManager::new(config.clone()));
        tracing::info!(
            "[LLM_STARTUP] ModelManager created in {:?}",
            manager_start.elapsed()
        );

        tracing::info!("[LLM_STARTUP] Setting up database...");
        let db_start = std::time::Instant::now();
        let database_path = config.database_path()?;
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LlmError::DatabaseError(format!("Failed to create database directory: {}", e))
            })?;
        }
        let database_url = format!("sqlite://{}", database_path.to_string_lossy());
        let database = Arc::new(LlmDatabase::new(&database_url).await?);
        tracing::info!(
            "[LLM_STARTUP] Database initialized in {:?}",
            db_start.elapsed()
        );

        let (events_tx, _) = broadcast::channel(128);
        let predictions_db = Arc::new(database.predictions.clone());

        // Build semantic engine from LiteRT if configured
        tracing::info!("[LLM_STARTUP] Building semantic provider...");
        let semantic_start = std::time::Instant::now();
        let semantic_provider = Self::build_semantic_provider(&config).await;
        tracing::info!(
            "[LLM_STARTUP] Semantic provider built in {:?}",
            semantic_start.elapsed()
        );

        tracing::info!("[LLM_STARTUP] Creating semantic engine...");
        let engine_start = std::time::Instant::now();
        let semantic_engine = if let Some(litert_lm) = semantic_provider {
            match SemanticEngine::new(litert_lm).await {
                Ok(engine) => {
                    tracing::info!(
                        "[LLM_STARTUP] Semantic engine created in {:?}",
                        engine_start.elapsed()
                    );
                    Some(Arc::new(engine))
                }
                Err(e) => {
                    tracing::warn!(
                        "[LLM_STARTUP] Failed to initialize semantic engine (took {:?}): {}",
                        engine_start.elapsed(),
                        e
                    );
                    None
                }
            }
        } else {
            tracing::info!(
                "[LLM_STARTUP] No semantic provider available (took {:?})",
                engine_start.elapsed()
            );
            None
        };

        tracing::info!("[LLM_STARTUP] Creating tool predictor...");
        let predictor_start = std::time::Instant::now();
        let tool_predictor = Arc::new(ToolPredictor::with_semantic(
            predictions_db.clone(),
            semantic_engine.clone(),
        ));
        tracing::info!(
            "[LLM_STARTUP] Tool predictor created in {:?}",
            predictor_start.elapsed()
        );

        tracing::info!("[LLM_STARTUP] Creating GEPA optimizer and session manager...");
        let session_start = std::time::Instant::now();
        let gepa_optimizer = Arc::new(GEPAOptimizer::new(None, &database));
        let session_manager = Arc::new(SessionManager::new(
            tool_predictor.clone(),
            Some(gepa_optimizer.clone()),
        ));
        tracing::info!(
            "[LLM_STARTUP] Session manager created in {:?}",
            session_start.elapsed()
        );

        tracing::info!("[LLM_STARTUP] Creating LlmService struct...");
        let struct_start = std::time::Instant::now();
        let service = Arc::new(Self {
            model_manager: manager.clone(),
            database,
            events_tx,
            tool_predictor,
            session_manager,
        });
        tracing::info!(
            "[LLM_STARTUP] LlmService struct created in {:?}",
            struct_start.elapsed()
        );

        // Forward model status updates into the service-wide event stream.
        tracing::info!("[LLM_STARTUP] Installing model event forwarder...");
        let forwarder_start = std::time::Instant::now();
        service.install_model_event_forwarder(manager);
        tracing::info!(
            "[LLM_STARTUP] Model event forwarder installed in {:?}",
            forwarder_start.elapsed()
        );

        Ok(service)
    }

    /// Ensure that at least one model is available (download + load if needed).
    pub async fn ensure_model_available(&self) -> LlmResult<()> {
        self.model_manager.ensure_model_available().await
    }

    /// Warm a session to minimise time-to-first-token.
    pub async fn warm_session(&self) -> LlmResult<()> {
        self.model_manager.warm_session().await
    }

    /// Obtain the current model status.
    pub async fn model_status(&self) -> ModelStatus {
        self.model_manager.get_model_status().await
    }

    /// Obtain the latest download progress (if any).
    pub async fn download_progress(&self) -> Option<DownloadProgress> {
        self.model_manager.get_download_progress().await
    }

    /// Access the underlying LLM database handle.
    pub fn database(&self) -> Arc<LlmDatabase> {
        self.database.clone()
    }

    /// Convenience clone of the predictions database handle.
    pub fn predictions_database(&self) -> PredictionsDatabase {
        self.database.predictions.clone()
    }

    /// Convenience clone of the GEPA database handle.
    pub fn gepa_database(&self) -> GepaDatabase {
        self.database.gepa.clone()
    }

    /// Create a heuristic tool predictor backed by the shared SQLite database.
    pub fn tool_predictor(&self) -> Arc<ToolPredictor> {
        self.tool_predictor.clone()
    }

    /// Access the shared session manager used for accuracy tracking.
    pub fn session_manager(&self) -> Arc<SessionManager> {
        self.session_manager.clone()
    }

    /// Subscribe to service events (model status changes, generation metrics, ...).
    pub fn subscribe_events(&self) -> broadcast::Receiver<LlmEvent> {
        self.events_tx.subscribe()
    }

    /// List cached models (delegates to ModelManager)
    pub async fn list_cached_models(&self) -> LlmResult<Vec<crate::model_management::ModelInfo>> {
        self.model_manager.list_cached_models().await
    }

    /// List available models (delegates to ModelManager)
    pub async fn list_available_models(
        &self,
    ) -> LlmResult<Vec<crate::model_management::ModelInfo>> {
        self.model_manager.list_available_models().await
    }

    /// Download a model (delegates to ModelManager)
    pub async fn download_model(&self, model_name: &str) -> LlmResult<DownloadProgress> {
        self.model_manager.download_model(model_name).await
    }

    /// Record the outcome of a code generation attempt for GEPA analytics.
    pub async fn record_codegen_attempt(
        &self,
        context_hash: &str,
        success: bool,
        metadata: Value,
    ) -> LlmResult<()> {
        let label = if success {
            "codegen_success"
        } else {
            "codegen_failure"
        };

        let prediction_id = self
            .database
            .predictions
            .record_prediction(
                context_hash,
                label,
                if success { 1.0 } else { 0.0 },
                metadata,
            )
            .await?;

        self.database
            .predictions
            .update_prediction_result(&prediction_id, label)
            .await?;

        Ok(())
    }

    /// Start a streaming generation request.
    ///
    /// Returns a handle that yields token events followed by completion metrics.
    pub async fn start_generation(
        &self,
        request: GenerationRequest,
    ) -> LlmResult<GenerationHandle> {
        // Ensure an engine is present.
        let engine = self
            .model_manager
            .get_engine()
            .await
            .ok_or_else(|| LlmError::ConfigError("No model engine available".to_string()))?;

        // Spin up a new conversation per request.
        let conversation = engine.create_conversation()?;

        // Kick off streaming.
        let mut stream = generate_streaming(&conversation, &request.prompt).await?;
        let (tx, rx) = mpsc::channel(128);
        let events_tx = self.events_tx.clone();
        let request_metadata = request.metadata();

        tokio::spawn(async move {
            let start = Instant::now();
            let mut first_token_at: Option<Instant> = None;
            let mut token_count: usize = 0;
            let mut combined_text = String::new();

            // Stream tokens forward.
            while let Some(token) = stream.next().await {
                if first_token_at.is_none() {
                    first_token_at = Some(Instant::now());
                }

                token_count += 1;
                combined_text.push_str(&token);

                if tx.send(GenerationEvent::Token(token)).await.is_err() {
                    return;
                }
            }

            let total_elapsed = start.elapsed();
            let ttft = first_token_at.map(|inst| inst.duration_since(start));
            let tokens_per_sec = if token_count > 0 && total_elapsed.as_secs_f64() > 0.0 {
                Some(token_count as f64 / total_elapsed.as_secs_f64())
            } else {
                None
            };

            // Try to get benchmark info from the conversation
            let benchmark_info = conversation.get_benchmark_info().ok();

            let metrics = if let Some(benchmark) = benchmark_info {
                // Use benchmark data from LiteRT engine
                GenerationMetrics::from_benchmark(benchmark)
            } else {
                // Fallback to manual timing
                GenerationMetrics {
                    total_tokens: token_count,
                    duration: total_elapsed,
                    time_to_first_token: ttft,
                    tokens_per_second: tokens_per_sec,
                    benchmark_info: None,
                }
            };

            let _ = tx.send(GenerationEvent::Completed(metrics.clone())).await;
            let _ = events_tx.send(LlmEvent::GenerationFinished {
                request: request_metadata,
                metrics,
                full_text: combined_text,
            });
        });

        Ok(GenerationHandle { events: rx })
    }

    fn install_model_event_forwarder(self: &Arc<Self>, manager: Arc<ModelManager>) {
        let mut receiver = manager.subscribe_status();
        let events_tx = self.events_tx.clone();
        tokio::spawn(async move {
            while let Ok(update) = receiver.recv().await {
                let event = match update {
                    ModelStatusUpdate::StatusChanged(status) => LlmEvent::ModelStatus(status),
                    ModelStatusUpdate::DownloadProgress(progress) => {
                        LlmEvent::DownloadProgress(progress)
                    }
                    ModelStatusUpdate::ModelReady(name, path) => {
                        LlmEvent::ModelReady { name, path }
                    }
                };

                let _ = events_tx.send(event);
            }
        });
    }
}

impl LlmService {
    async fn build_semantic_provider(config: &AppConfig) -> Option<Arc<LiteRTLM>> {
        if !config.llm.semantic_routing {
            return None;
        }

        let model_path = match config.llm.semantic_model_path() {
            Ok(Some(path)) => path,
            Ok(None) => {
                warn!("semantic_routing enabled but no semantic_model_path configured");
                return None;
            }
            Err(e) => {
                warn!(
                    "Failed to resolve semantic model path (semantic routing disabled): {}",
                    e
                );
                return None;
            }
        };

        let backend = match config.backend() {
            Ok(backend) => backend,
            Err(e) => {
                warn!(
                    "Failed to determine LiteRT backend (semantic routing disabled): {}",
                    e
                );
                return None;
            }
        };

        let builder = LiteRTBuilder::new()
            .model_path(model_path.to_string_lossy().into_owned())
            .backend(backend)
            .temperature(config.llm.temperature)
            .max_tokens(config.llm.max_tokens);

        match builder.build().await {
            Ok(provider) => Some(Arc::new(provider)),
            Err(e) => {
                warn!(
                    "Unable to initialize semantic LiteRT provider (semantic routing disabled): {}",
                    e
                );
                None
            }
        }
    }
}

/// Parameters for initiating a streamed generation.
#[derive(Debug, Clone)]
pub struct GenerationRequest {
    /// Prompt text to submit to the model.
    pub prompt: String,
    /// Optional decoding temperature override.
    pub temperature: Option<f32>,
    /// Optional maximum tokens to generate.
    pub max_tokens: Option<usize>,
}

impl GenerationRequest {
    fn metadata(&self) -> GenerationRequestMetadata {
        GenerationRequestMetadata {
            temperature: self.temperature,
            max_tokens: self.max_tokens,
        }
    }
}

/// Metadata persisted alongside generation events (for diagnostics).
#[derive(Debug, Clone)]
pub struct GenerationRequestMetadata {
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
}

/// Handle returned to consumers for reading streamed generation events.
pub struct GenerationHandle {
    events: mpsc::Receiver<GenerationEvent>,
}

impl GenerationHandle {
    /// Receive the next generation event (token or completion).
    pub async fn next(&mut self) -> Option<GenerationEvent> {
        self.events.recv().await
    }

    /// Consume the handle and return the underlying receiver.
    pub fn into_inner(self) -> mpsc::Receiver<GenerationEvent> {
        self.events
    }
}

/// Event emitted while consuming a streamed generation.
#[derive(Debug, Clone)]
pub enum GenerationEvent {
    /// A new token is available.
    Token(String),
    /// Generation completed with metrics.
    Completed(GenerationMetrics),
}

/// Diagnostics about a finished generation.
#[derive(Debug, Clone)]
pub struct GenerationMetrics {
    pub total_tokens: usize,
    pub duration: std::time::Duration,
    pub time_to_first_token: Option<std::time::Duration>,
    pub tokens_per_second: Option<f64>,
    /// Full benchmark data from LiteRT engine (if available)
    pub benchmark_info: Option<crate::litert_wrapper::BenchmarkInfo>,
}

impl GenerationMetrics {
    /// Create GenerationMetrics from LiteRT BenchmarkInfo
    pub fn from_benchmark(benchmark: crate::litert_wrapper::BenchmarkInfo) -> Self {
        Self {
            total_tokens: benchmark.total_tokens() as usize,
            duration: std::time::Duration::from_secs_f64(benchmark.total_duration_seconds()),
            time_to_first_token: Some(std::time::Duration::from_millis(
                benchmark.time_to_first_token_ms as u64,
            )),
            tokens_per_second: Some(benchmark.overall_tokens_per_sec()),
            benchmark_info: Some(benchmark),
        }
    }
}

/// High-level events emitted by the service.
#[derive(Debug, Clone)]
pub enum LlmEvent {
    ModelStatus(ModelStatus),
    DownloadProgress(DownloadProgress),
    ModelReady {
        name: String,
        path: std::path::PathBuf,
    },
    GenerationFinished {
        request: GenerationRequestMetadata,
        metrics: GenerationMetrics,
        full_text: String,
    },
}
