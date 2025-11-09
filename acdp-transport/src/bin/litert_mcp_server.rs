//! LiteRT MCP Server
//!
//! HTTP-SSE MCP server that exposes LiteRT-LM capabilities as MCP tools.
//! This server provides:
//! - text_generation: Generate text using on-device LLM
//! - tool_prediction: Predict which tool to use for a query
//! - model_info: Get information about loaded models
//! - cache_status: Check model cache status

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{sse::Event, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use futures_util::stream::Stream;
use acdp_core::messages::{
    Capabilities, Implementation, InitializeResponse, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, RequestId, StandardCapabilities, Tool, ToolCapabilities,
};
use acdp_llm::{AppConfig, GenerationRequest, LlmService, ModelStatus};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "litert-mcp-server")]
#[command(about = "HTTP-SSE MCP server exposing LiteRT-LM capabilities")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "8084")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// Session state for each connected client
#[derive(Clone)]
struct Session {
    id: String,
    initialized: bool,
    sse_tx: broadcast::Sender<JsonRpcMessage>,
}

/// Shared server state
#[derive(Clone)]
struct ServerState {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    llm_service: Arc<RwLock<Option<Arc<LlmService>>>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            llm_service: Arc::new(RwLock::new(None)),
        }
    }

    async fn set_llm_service(&self, service: Arc<LlmService>) {
        let mut llm = self.llm_service.write().await;
        *llm = Some(service);
    }

    async fn get_or_create_session(&self, session_id: Option<String>) -> Session {
        let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut sessions = self.sessions.lock().await;

        if let Some(session) = sessions.get(&session_id) {
            session.clone()
        } else {
            let (tx, _) = broadcast::channel(100);
            let session = Session {
                id: session_id.clone(),
                initialized: false,
                sse_tx: tx,
            };
            sessions.insert(session_id.clone(), session.clone());
            session
        }
    }

    async fn mark_initialized(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.initialized = true;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(format!(
                    "litert_mcp_server={},mcp_llm={},mcp_core={}",
                    log_level, log_level, log_level
                ))
            }),
        )
        .init();

    info!("Starting LiteRT MCP Server");
    info!("  Listening on: {}:{}", args.host, args.port);

    // Initialize LLM service in background
    let state = ServerState::new();
    let state_clone = state.clone();

    tokio::spawn(async move {
        info!("Initializing LiteRT LLM service...");
        match AppConfig::load() {
            Ok(config) => match LlmService::new(config).await {
                Ok(service) => {
                    info!("LiteRT LLM service initialized successfully");
                    state_clone.set_llm_service(service).await;
                }
                Err(e) => {
                    error!("Failed to initialize LLM service: {}", e);
                }
            },
            Err(e) => {
                error!("Failed to load LLM config: {}", e);
            }
        }
    });

    // Create router
    let app = Router::new()
        .route("/sse", post(handle_sse_post))
        .route("/sse", get(handle_sse_get))
        .with_state(state);

    // Start server
    let bind_addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    info!("LiteRT MCP server starting on {}", bind_addr);
    info!("Clients can connect to: http://{}/sse", bind_addr);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .context("Failed to bind HTTP server")?;

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}

/// Handle HTTP POST to /sse - MCP request/response
async fn handle_sse_post(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response<axum::body::Body>, StatusCode> {
    debug!("Received SSE POST request: {:?}", body);

    // Parse JSON-RPC message
    let message: JsonRpcMessage = match serde_json::from_value(body) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse JSON-RPC message: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Extract session ID
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let session = state.get_or_create_session(session_id).await;

    // Process message
    let response = match message {
        JsonRpcMessage::Request(req) => handle_request(&state, &session, req).await,
        JsonRpcMessage::Notification(notif) => {
            handle_notification(&state, &session, notif).await;
            // Notifications don't have responses
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: RequestId::Null,
                result: Some(json!({"status": "ok"})),
                error: None,
            }
        }
        _ => {
            error!("Unexpected message type");
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Return as SSE event
    let sse_json =
        serde_json::to_string(&response).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let sse_data = format!("data: {}\n\n", sse_json);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Mcp-Session-Id", session.id.clone())
        .body(axum::body::Body::from(sse_data))
        .unwrap())
}

/// Handle HTTP GET to /sse - establish persistent SSE connection
async fn handle_sse_get(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let session = state.get_or_create_session(session_id).await;
    info!("SSE GET connection established: session {}", session.id);

    let mut sse_rx = session.sse_tx.subscribe();

    let stream = async_stream::stream! {
        // Send connection established event
        yield Ok(Event::default()
            .event("connected")
            .data(format!(r#"{{"session_id":"{}"}}"#, session.id)));

        // Stream messages
        loop {
            match sse_rx.recv().await {
                Ok(message) => {
                    if let Ok(json) = serde_json::to_string(&message) {
                        yield Ok(Event::default().data(json));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SSE client {} lagged, skipped {} messages", session.id, skipped);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("SSE stream closed for session {}", session.id);
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}

async fn handle_request(
    state: &ServerState,
    session: &Session,
    req: JsonRpcRequest,
) -> JsonRpcResponse {
    debug!("Handling request: {}", req.method);

    let result = match req.method.as_str() {
        "initialize" => handle_initialize(state, session, req.params).await,
        "tools/list" => handle_tools_list().await,
        "tools/call" => handle_tools_call(state, req.params).await,
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err(json!({
            "code": -32601,
            "message": format!("Method not found: {}", req.method)
        })),
    };

    match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: None,
            error: Some(acdp_core::messages::JsonRpcError {
                code: error["code"].as_i64().unwrap_or(-32603) as i32,
                message: error["message"]
                    .as_str()
                    .unwrap_or("Internal error")
                    .to_string(),
                data: None,
            }),
        },
    }
}

async fn handle_notification(state: &ServerState, session: &Session, notif: JsonRpcNotification) {
    debug!("Handling notification: {}", notif.method);

    match notif.method.as_str() {
        "initialized" => {
            state.mark_initialized(&session.id).await;
            info!("Session {} initialized", session.id);
        }
        _ => {
            warn!("Unknown notification method: {}", notif.method);
        }
    }
}

async fn handle_initialize(
    _state: &ServerState,
    _session: &Session,
    _params: Option<Value>,
) -> Result<Value, Value> {
    info!("Handling initialize request");

    let result = InitializeResponse {
        protocol_version: acdp_core::ProtocolVersion::V2024_11_05,
        capabilities: Capabilities {
            standard: StandardCapabilities {
                tools: Some(ToolCapabilities {
                    list_changed: Some(false),
                }),
                resources: None,
                prompts: None,
                sampling: None,
                logging: None,
                roots: None,
            },
            custom: HashMap::new(),
        },
        server_info: Implementation::new("litert-mcp-server", env!("CARGO_PKG_VERSION")),
        instructions: Some(
            "LiteRT MCP Server - On-device LLM capabilities via MCP protocol".to_string(),
        ),
    };

    Ok(serde_json::to_value(result).unwrap())
}

async fn handle_tools_list() -> Result<Value, Value> {
    info!("Handling tools/list request");

    let tools = vec![
        Tool {
            name: "text_generation".to_string(),
            description: "Generate text using on-device LLM (LiteRT)".to_string(),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The prompt for text generation"
                    },
                    "temperature": {
                        "type": "number",
                        "description": "Sampling temperature (0.0-1.0)",
                        "default": 0.7
                    },
                    "max_tokens": {
                        "type": "number",
                        "description": "Maximum tokens to generate",
                        "default": 512
                    }
                },
                "required": ["prompt"]
            })),
            extensions: None,
            read_only: Some(false),
            return_type: None,
        },
        Tool {
            name: "tool_prediction".to_string(),
            description: "Predict which tool to use for a query using LiteRT".to_string(),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The user query to predict tool for"
                    },
                    "available_tools": {
                        "type": "array",
                        "description": "List of available tool names",
                        "items": { "type": "string" }
                    }
                },
                "required": ["query", "available_tools"]
            })),
            extensions: None,
            read_only: Some(true),
            return_type: None,
        },
        Tool {
            name: "model_info".to_string(),
            description: "Get information about the loaded LiteRT model".to_string(),
            input_schema: Some(json!({
                "type": "object",
                "properties": {}
            })),
            extensions: None,
            read_only: Some(true),
            return_type: None,
        },
        Tool {
            name: "cache_status".to_string(),
            description: "Check the status of the model cache".to_string(),
            input_schema: Some(json!({
                "type": "object",
                "properties": {}
            })),
            extensions: None,
            read_only: Some(true),
            return_type: None,
        },
    ];

    Ok(json!({ "tools": tools }))
}

async fn handle_tools_call(state: &ServerState, params: Option<Value>) -> Result<Value, Value> {
    let params = params.ok_or_else(|| {
        json!({
            "code": -32602,
            "message": "Missing params"
        })
    })?;

    let tool_name = params["name"].as_str().ok_or_else(|| {
        json!({
            "code": -32602,
            "message": "Missing tool name"
        })
    })?;

    let arguments = params["arguments"].clone();

    info!("Calling tool: {}", tool_name);
    debug!("Tool arguments: {:?}", arguments);

    match tool_name {
        "text_generation" => handle_text_generation(state, arguments).await,
        "tool_prediction" => handle_tool_prediction(state, arguments).await,
        "model_info" => handle_model_info(state).await,
        "cache_status" => handle_cache_status(state).await,
        _ => Err(json!({
            "code": -32602,
            "message": format!("Unknown tool: {}", tool_name)
        })),
    }
}

async fn handle_text_generation(state: &ServerState, args: Value) -> Result<Value, Value> {
    let llm_service = state.llm_service.read().await;
    let service = llm_service.as_ref().ok_or_else(|| {
        json!({
            "code": -32603,
            "message": "LLM service not initialized"
        })
    })?;

    let prompt = args["prompt"].as_str().ok_or_else(|| {
        json!({
            "code": -32602,
            "message": "Missing prompt"
        })
    })?;

    let temperature = args["temperature"].as_f64().map(|t| t as f32);
    let max_tokens = args["max_tokens"].as_u64().map(|t| t as usize);

    let request = GenerationRequest {
        prompt: prompt.to_string(),
        temperature,
        max_tokens,
    };

    info!("Generating text for prompt: {}", prompt);

    match service.start_generation(request).await {
        Ok(mut handle) => {
            let mut accumulated = String::new();

            while let Some(event) = handle.next().await {
                match event {
                    acdp_llm::GenerationEvent::Token(token) => {
                        accumulated.push_str(&token);
                    }
                    acdp_llm::GenerationEvent::Completed(metrics) => {
                        info!(
                            "Generation completed: {} tokens in {:.2}s",
                            accumulated.len(),
                            metrics
                                .time_to_first_token
                                .map(|d| d.as_secs_f64())
                                .unwrap_or(0.0)
                        );
                        break;
                    }
                }
            }

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": accumulated
                }]
            }))
        }
        Err(e) => Err(json!({
            "code": -32603,
            "message": format!("Generation failed: {}", e)
        })),
    }
}

async fn handle_tool_prediction(state: &ServerState, args: Value) -> Result<Value, Value> {
    let llm_service = state.llm_service.read().await;
    let service = llm_service.as_ref().ok_or_else(|| {
        json!({
            "code": -32603,
            "message": "LLM service not initialized"
        })
    })?;

    let query = args["query"].as_str().ok_or_else(|| {
        json!({
            "code": -32602,
            "message": "Missing query"
        })
    })?;

    let available_tools: Vec<String> = args["available_tools"]
        .as_array()
        .ok_or_else(|| {
            json!({
                "code": -32602,
                "message": "Missing or invalid available_tools"
            })
        })?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    info!("Predicting tool for query: {}", query);

    // Build MCP context string with available tools
    let context = format!(
        "User query: {}\nAvailable tools: {}",
        query,
        available_tools.join(", ")
    );

    let predictor = service.tool_predictor();
    match predictor.predict_tool(&context).await {
        Ok(outcome) => {
            info!(
                "Predicted tool: {} (confidence: {:.2})",
                outcome.prediction.tool_name, outcome.prediction.confidence
            );

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "Predicted tool: {} (confidence: {:.0}%)\nReasoning: {}",
                        outcome.prediction.tool_name,
                        outcome.prediction.confidence * 100.0,
                        outcome.prediction.reasoning
                    )
                }],
                "prediction": {
                    "tool": outcome.prediction.tool_name,
                    "confidence": outcome.prediction.confidence,
                    "record_id": outcome.record_id
                }
            }))
        }
        Err(e) => Err(json!({
            "code": -32603,
            "message": format!("Prediction failed: {}", e)
        })),
    }
}

async fn handle_model_info(state: &ServerState) -> Result<Value, Value> {
    let llm_service = state.llm_service.read().await;
    let service = llm_service.as_ref().ok_or_else(|| {
        json!({
            "code": -32603,
            "message": "LLM service not initialized"
        })
    })?;

    let status = service.model_status().await;
    let info_text = format!("Model status: {:?}", status);

    Ok(json!({
        "content": [{
            "type": "text",
            "text": info_text
        }],
        "status": format!("{:?}", status)
    }))
}

async fn handle_cache_status(state: &ServerState) -> Result<Value, Value> {
    let llm_service = state.llm_service.read().await;
    let service = llm_service.as_ref().ok_or_else(|| {
        json!({
            "code": -32603,
            "message": "LLM service not initialized"
        })
    })?;

    // Get model status
    let status = service.model_status().await;
    let download_progress = service.download_progress().await;

    let info_text = match (&status, &download_progress) {
        (_, Some(progress)) => format!(
            "Model downloading: {} ({:.1}%)",
            progress.model_name, progress.percentage
        ),
        (ModelStatus::Ready, None) => "Model ready and loaded".to_string(),
        (ModelStatus::Loading, None) => "Model loading...".to_string(),
        (ModelStatus::NotLoaded, None) => "No model loaded".to_string(),
        (ModelStatus::Error(err), None) => format!("Model error: {}", err),
    };

    Ok(json!({
        "content": [{
            "type": "text",
            "text": info_text
        }],
        "status": format!("{:?}", status)
    }))
}
