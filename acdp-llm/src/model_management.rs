//! Model management for LiteRT-LM

use crate::config::AppConfig;
use crate::error::{LlmError, LlmResult};
use crate::litert_wrapper::{LiteRTBackend, LiteRTEngine};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

/// Model information
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub is_cached: bool,
}

/// Model status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStatus {
    NotLoaded,
    Loading,
    Ready,
    Error(String),
}

impl std::fmt::Display for ModelStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelStatus::NotLoaded => write!(f, "NotLoaded"),
            ModelStatus::Loading => write!(f, "Loading"),
            ModelStatus::Ready => write!(f, "Ready"),
            ModelStatus::Error(msg) => write!(f, "Error: {}", msg),
        }
    }
}

/// Download progress
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub model_name: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub percentage: f32,
}

/// Model manager that ensures at least one model is always available
pub struct ModelManager {
    config: AppConfig,
    current_model: Arc<RwLock<Option<Arc<LiteRTEngine>>>>,
    current_model_path: Arc<RwLock<Option<PathBuf>>>,
    status: Arc<RwLock<ModelStatus>>,
    download_progress: Arc<RwLock<Option<DownloadProgress>>>,
    status_tx: broadcast::Sender<ModelStatusUpdate>,
}

/// Model status update for real-time monitoring
#[derive(Debug, Clone)]
pub enum ModelStatusUpdate {
    StatusChanged(ModelStatus),
    DownloadProgress(DownloadProgress),
    ModelReady(String, PathBuf),
}

impl ModelManager {
    /// Create a new model manager
    pub fn new(config: AppConfig) -> Self {
        let (status_tx, _) = broadcast::channel(100);

        Self {
            config,
            current_model: Arc::new(RwLock::new(None)),
            current_model_path: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::NotLoaded)),
            download_progress: Arc::new(RwLock::new(None)),
            status_tx,
        }
    }

    /// Get a receiver for status updates
    pub fn subscribe_status(&self) -> broadcast::Receiver<ModelStatusUpdate> {
        self.status_tx.subscribe()
    }

    /// List available models from cache
    pub async fn list_cached_models(&self) -> LlmResult<Vec<ModelInfo>> {
        let cache_dir = self.config.cache_dir()?;

        if !cache_dir.exists() {
            return Ok(Vec::new());
        }

        let mut models = Vec::new();

        let entries = std::fs::read_dir(&cache_dir)
            .map_err(|e| LlmError::ConfigError(format!("Failed to read cache directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                LlmError::ConfigError(format!("Failed to read directory entry: {}", e))
            })?;

            let path = entry.path();

            // Check if it's a directory (models are typically in directories)
            if path.is_dir() {
                // Look for model files (common extensions: .gguf, .bin, etc.)
                if Self::is_model_directory(&path)? {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let size = Self::calculate_directory_size(&path)?;

                    models.push(ModelInfo {
                        name,
                        path,
                        size_bytes: size,
                        is_cached: true,
                    });
                }
            } else if Self::is_model_file(&path) {
                // Single file model
                let name = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let size = path.metadata().map(|m| m.len()).unwrap_or(0);

                models.push(ModelInfo {
                    name,
                    path,
                    size_bytes: size,
                    is_cached: true,
                });
            }
        }

        Ok(models)
    }

    /// List available models (prioritizes cached models, then popular models)
    pub async fn list_available_models(&self) -> LlmResult<Vec<ModelInfo>> {
        // Start with cached models (fast, always works)
        let mut models = self.list_cached_models().await?;

        // Add popular LiteRT models
        models.extend(Self::popular_litert_models());

        // Try to fetch from HuggingFace if token is available
        if let Ok(hf_token) = std::env::var("HF_TOKEN") {
            if !hf_token.is_empty() {
                if let Ok(hf_models) = Self::fetch_huggingface_models(&hf_token).await {
                    models.extend(hf_models);
                }
            }
        }

        // Optionally try to fetch from Kaggle API in background (don't block)
        // Note: This is disabled by default since it requires authentication
        // and can be slow. Enable by setting ENABLE_KAGGLE_FETCH=1
        if std::env::var("ENABLE_KAGGLE_FETCH").is_ok() {
            if let Ok(kaggle_models) = Self::fetch_kaggle_models().await {
                models.extend(kaggle_models);
            }
        }

        Ok(models)
    }

    /// Fetch LiteRT models from HuggingFace
    async fn fetch_huggingface_models(token: &str) -> LlmResult<Vec<ModelInfo>> {
        let client = reqwest::Client::new();

        // Query HuggingFace for gemma models with "litert" or "int4" in tags
        let response = client
            .get("https://huggingface.co/api/models")
            .query(&[("search", "gemma"), ("filter", "litert"), ("limit", "20")])
            .header("Authorization", format!("Bearer {}", token))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                LlmError::ConfigError(format!("Failed to fetch HuggingFace models: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(LlmError::ConfigError(format!(
                "HuggingFace API returned status: {}",
                response.status()
            )));
        }

        let models_json: serde_json::Value = response.json().await.map_err(|e| {
            LlmError::ConfigError(format!("Failed to parse HuggingFace response: {}", e))
        })?;

        let mut models = Vec::new();

        if let Some(items) = models_json.as_array() {
            for item in items {
                if let Some(model_id) = item.get("id").and_then(|s| s.as_str()) {
                    // Check if it's a LiteRT-compatible model
                    let tags = item.get("tags").and_then(|t| t.as_array());
                    let is_litert = tags
                        .map(|tags| {
                            tags.iter().any(|t| {
                                t.as_str()
                                    .map(|s| {
                                        s.contains("litert")
                                            || s.contains("int4")
                                            || s.contains("tflite")
                                    })
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false);

                    if is_litert || model_id.contains("litert") || model_id.contains("int4") {
                        models.push(ModelInfo {
                            name: model_id.to_string(),
                            path: PathBuf::new(),
                            size_bytes: 0, // Size not provided in list API
                            is_cached: false,
                        });
                    }
                }
            }
        }

        Ok(models)
    }

    /// Fetch models from Kaggle Models API
    async fn fetch_kaggle_models() -> LlmResult<Vec<ModelInfo>> {
        let client = reqwest::Client::new();

        // Query Kaggle Models API for LiteRT-compatible models
        let response = client
            .get("https://www.kaggle.com/api/v1/models/list")
            .query(&[("search", "litert gemma"), ("pageSize", "20")])
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| LlmError::ConfigError(format!("Failed to fetch Kaggle models: {}", e)))?;

        if !response.status().is_success() {
            return Err(LlmError::ConfigError(format!(
                "Kaggle API returned status: {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response.json().await.map_err(|e| {
            LlmError::ConfigError(format!("Failed to parse Kaggle response: {}", e))
        })?;

        let mut models = Vec::new();

        if let Some(items) = json.get("models").and_then(|m| m.as_array()) {
            for item in items {
                if let (Some(name), Some(owner)) = (
                    item.get("slug").and_then(|s| s.as_str()),
                    item.get("ownerSlug").and_then(|s| s.as_str()),
                ) {
                    let full_name = format!("{}/{}", owner, name);
                    let size = item.get("totalBytes").and_then(|s| s.as_u64()).unwrap_or(0);

                    models.push(ModelInfo {
                        name: full_name,
                        path: PathBuf::new(), // Not downloaded yet
                        size_bytes: size,
                        is_cached: false,
                    });
                }
            }
        }

        Ok(models)
    }

    /// List of popular LiteRT-compatible models (matches lit binary registry)
    pub fn popular_litert_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                name: "gemma3-1b".to_string(),
                path: PathBuf::new(),
                size_bytes: 584_417_280, // ~557MB (int4 quantized)
                is_cached: false,
            },
            ModelInfo {
                name: "gemma-3n-E2B".to_string(),
                path: PathBuf::new(),
                size_bytes: 2_500_000_000, // ~2.3GB estimated
                is_cached: false,
            },
            ModelInfo {
                name: "gemma-3n-E4B".to_string(),
                path: PathBuf::new(),
                size_bytes: 4_500_000_000, // ~4.2GB estimated
                is_cached: false,
            },
            ModelInfo {
                name: "phi-4-mini".to_string(),
                path: PathBuf::new(),
                size_bytes: 3_800_000_000, // ~3.5GB estimated
                is_cached: false,
            },
            ModelInfo {
                name: "qwen2.5-1.5b".to_string(),
                path: PathBuf::new(),
                size_bytes: 1_600_000_000, // ~1.5GB estimated
                is_cached: false,
            },
        ]
    }

    /// Get the smallest available model
    pub async fn get_smallest_model(&self) -> LlmResult<Option<ModelInfo>> {
        let models = self.list_available_models().await?;

        if models.is_empty() {
            return Ok(None);
        }

        let smallest = models.iter().min_by_key(|m| m.size_bytes).cloned();

        Ok(smallest)
    }

    /// Download a model from Kaggle or HuggingFace
    pub async fn download_model(&self, model_name: &str) -> LlmResult<DownloadProgress> {
        use futures_util::StreamExt;

        // Set status to loading
        *self.status.write().await = ModelStatus::Loading;
        let _ = self
            .status_tx
            .send(ModelStatusUpdate::StatusChanged(ModelStatus::Loading));

        // Determine download URL based on model name
        let download_url = if model_name.starts_with("google/") {
            // Kaggle Models download URL
            format!("https://www.kaggle.com/models/{}/download", model_name)
        } else {
            // HuggingFace download URL (for .gguf files)
            format!(
                "https://huggingface.co/{}/resolve/main/model.gguf",
                model_name
            )
        };

        // Create cache directory if needed
        let cache_dir = self.config.cache_dir()?;
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| LlmError::ConfigError(format!("Failed to create cache dir: {}", e)))?;

        // Sanitize model name for file path
        let safe_name = model_name.replace('/', "_");
        let model_path = cache_dir.join(&safe_name);

        // Start download
        let client = reqwest::Client::new();
        let response = client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| LlmError::ConfigError(format!("Failed to start download: {}", e)))?;

        if !response.status().is_success() {
            return Err(LlmError::ConfigError(format!(
                "Download failed with status: {}",
                response.status()
            )));
        }

        let total_bytes = response.content_length();
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();

        // Create temporary file
        let temp_path = model_path.with_extension("tmp");
        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|e| LlmError::ConfigError(format!("Failed to create temp file: {}", e)))?;

        // Download with progress tracking
        use tokio::io::AsyncWriteExt;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| LlmError::ConfigError(format!("Download stream error: {}", e)))?;

            file.write_all(&chunk)
                .await
                .map_err(|e| LlmError::ConfigError(format!("Failed to write chunk: {}", e)))?;

            downloaded += chunk.len() as u64;

            // Calculate percentage
            let percentage = if let Some(total) = total_bytes {
                (downloaded as f32 / total as f32) * 100.0
            } else {
                0.0
            };

            // Send progress update
            let progress = DownloadProgress {
                model_name: model_name.to_string(),
                bytes_downloaded: downloaded,
                total_bytes,
                percentage,
            };

            *self.download_progress.write().await = Some(progress.clone());
            let _ = self
                .status_tx
                .send(ModelStatusUpdate::DownloadProgress(progress.clone()));
        }

        // Finalize download
        file.flush()
            .await
            .map_err(|e| LlmError::ConfigError(format!("Failed to flush file: {}", e)))?;
        drop(file);

        // Rename temp file to final name
        tokio::fs::rename(&temp_path, &model_path)
            .await
            .map_err(|e| LlmError::ConfigError(format!("Failed to rename file: {}", e)))?;

        // Clear progress and set status
        *self.download_progress.write().await = None;
        *self.status.write().await = ModelStatus::Ready;
        let _ = self
            .status_tx
            .send(ModelStatusUpdate::StatusChanged(ModelStatus::Ready));
        let _ = self.status_tx.send(ModelStatusUpdate::ModelReady(
            model_name.to_string(),
            model_path.clone(),
        ));

        Ok(DownloadProgress {
            model_name: model_name.to_string(),
            bytes_downloaded: downloaded,
            total_bytes,
            percentage: 100.0,
        })
    }

    /// Ensure at least one model is available
    pub async fn ensure_model_available(&self) -> LlmResult<()> {
        // Check if we already have a model loaded
        {
            let model = self.current_model.read().await;
            if model.is_some() {
                return Ok(());
            }
        }

        // First, check if semantic_model_path is configured and exists
        if let Ok(Some(semantic_path)) = self.config.llm.semantic_model_path() {
            if semantic_path.exists() {
                tracing::info!(
                    "Loading model from semantic_model_path: {:?}",
                    semantic_path
                );
                return self.load_model(&semantic_path).await;
            } else {
                tracing::warn!(
                    "Configured semantic_model_path does not exist: {:?}",
                    semantic_path
                );
            }
        }

        // Check cache second
        let cached_models = self.list_cached_models().await?;

        if !cached_models.is_empty() {
            // Use the first cached model (or preferred if available)
            let model_to_use = if let Some(preferred) = &self.config.model.preferred_model {
                cached_models
                    .iter()
                    .find(|m| &m.name == preferred)
                    .or_else(|| cached_models.first())
            } else {
                cached_models.first()
            };

            if let Some(model_info) = model_to_use {
                return self.load_model(&model_info.path).await;
            }
        }

        // No cached model, check settings for preference
        let model_to_download = if let Some(preferred) = &self.config.model.preferred_model {
            preferred.clone()
        } else {
            // Get smallest available model
            if let Some(smallest) = self.get_smallest_model().await? {
                smallest.name
            } else {
                return Err(LlmError::ConfigError("No models available".to_string()));
            }
        };

        // Download the model
        self.download_model(&model_to_download).await?;

        // Load the downloaded model
        let cache_dir = self.config.cache_dir()?;
        let model_path = cache_dir.join(&model_to_download);
        self.load_model(&model_path).await
    }

    /// Load a model from path
    pub async fn load_model(&self, model_path: &Path) -> LlmResult<()> {
        *self.status.write().await = ModelStatus::Loading;
        let _ = self
            .status_tx
            .send(ModelStatusUpdate::StatusChanged(ModelStatus::Loading));

        let backend = self.config.backend()?;
        let model_path_str = model_path.to_string_lossy().to_string();

        // Try GPU backend first, fallback to CPU if it fails
        let engine_result = match backend {
            LiteRTBackend::Gpu => {
                tracing::info!("Attempting to load model with GPU backend");
                match LiteRTEngine::new(&model_path_str, LiteRTBackend::Gpu) {
                    Ok(engine) => {
                        tracing::info!("Successfully loaded model with GPU backend");
                        Ok(engine)
                    }
                    Err(gpu_err) => {
                        tracing::warn!(
                            "GPU backend failed: {}. Falling back to CPU backend",
                            gpu_err
                        );
                        LiteRTEngine::new(&model_path_str, LiteRTBackend::Cpu).map_err(|cpu_err| {
                            LlmError::RuntimeError(format!(
                                "GPU backend failed: {}. CPU fallback also failed: {}",
                                gpu_err, cpu_err
                            ))
                        })
                    }
                }
            }
            LiteRTBackend::Cpu => {
                tracing::info!("Loading model with CPU backend");
                LiteRTEngine::new(&model_path_str, LiteRTBackend::Cpu)
            }
        };

        match engine_result {
            Ok(engine) => {
                *self.current_model.write().await = Some(Arc::new(engine));
                *self.current_model_path.write().await = Some(model_path.to_path_buf());
                *self.status.write().await = ModelStatus::Ready;
                let _ = self
                    .status_tx
                    .send(ModelStatusUpdate::StatusChanged(ModelStatus::Ready));
                let _ = self.status_tx.send(ModelStatusUpdate::ModelReady(
                    model_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    model_path.to_path_buf(),
                ));
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to load model: {}", e);
                *self.status.write().await = ModelStatus::Error(error_msg.clone());
                let _ = self
                    .status_tx
                    .send(ModelStatusUpdate::StatusChanged(ModelStatus::Error(
                        error_msg.clone(),
                    )));
                Err(LlmError::RuntimeError(error_msg))
            }
        }
    }

    /// Get current model status
    pub async fn get_model_status(&self) -> ModelStatus {
        self.status.read().await.clone()
    }

    /// Get current download progress
    pub async fn get_download_progress(&self) -> Option<DownloadProgress> {
        self.download_progress.read().await.clone()
    }

    /// Get the current model engine (if loaded)
    pub async fn get_engine(&self) -> Option<Arc<LiteRTEngine>> {
        self.current_model.read().await.clone()
    }

    /// Warm a session (create and immediately use it to ensure instant token availability)
    pub async fn warm_session(&self) -> LlmResult<()> {
        let model = self.current_model.read().await;
        if let Some(ref engine) = *model {
            // Create a conversation and do a minimal generation to warm it up
            let conversation = engine.create_conversation()?;
            // Do a tiny generation to warm up
            let _ = conversation.send_user_message("Hi")?;
            Ok(())
        } else {
            Err(LlmError::ConfigError("No model loaded".to_string()))
        }
    }

    /// Check if a directory contains a model
    fn is_model_directory(path: &Path) -> LlmResult<bool> {
        // Check for common model file extensions
        let entries = std::fs::read_dir(path)
            .map_err(|e| LlmError::ConfigError(format!("Failed to read directory: {}", e)))?;

        for entry in entries {
            let entry =
                entry.map_err(|e| LlmError::ConfigError(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();
            if Self::is_model_file(&path) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Check if a file is a model file
    fn is_model_file(path: &Path) -> bool {
        // Get filename to check for shard patterns
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Exclude .litertlm shard files (pattern: *.litertlm_<number>.bin)
        if filename.contains(".litertlm_") && filename.ends_with(".bin") {
            return false;
        }

        if let Some(ext) = path.extension() {
            let ext_lower = ext.to_string_lossy().to_lowercase();
            matches!(
                ext_lower.as_str(),
                "gguf" | "safetensors" | "pt" | "pth" | "onnx" | "litertlm"
            )
        } else {
            false
        }
    }

    /// Calculate directory size recursively
    fn calculate_directory_size(path: &Path) -> LlmResult<u64> {
        let mut total = 0u64;

        if path.is_file() {
            return Ok(path.metadata().map(|m| m.len()).unwrap_or(0));
        }

        let entries = std::fs::read_dir(path)
            .map_err(|e| LlmError::ConfigError(format!("Failed to read directory: {}", e)))?;

        for entry in entries {
            let entry =
                entry.map_err(|e| LlmError::ConfigError(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            if path.is_dir() {
                total += Self::calculate_directory_size(&path)?;
            } else {
                total += path.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }

        Ok(total)
    }
}
