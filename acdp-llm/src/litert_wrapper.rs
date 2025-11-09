//! LiteRT-LM Rust wrapper providing safe interface to the C API
//!
//! This uses the LiteRT-LM Rust API built from Bazel.
//!
#![cfg_attr(litert_stub, allow(dead_code, unused_imports))]

use crate::error::{LlmError, LlmResult};
use serde::{Deserialize, Serialize};
#[cfg(litert_dynamic)]
use std::ffi::{CStr, CString};
#[cfg(litert_dynamic)]
use std::os::raw::{c_char, c_int};

#[cfg(all(not(litert_dynamic), not(litert_stub)))]
compile_error!("Either `litert_dynamic` or `litert_stub` cfg must be set in build.rs");

// ============================================================================
// FFI Declarations
// ============================================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum LiteRtLmBackendFFI {
    Cpu = 0,
    Gpu = 1,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum LiteRtLmStatusFFI {
    Ok = 0,
    Error = -1,
    ErrorInvalidArgs = -2,
    ErrorNotInitialized = -3,
    ErrorModelLoadFailed = -4,
    ErrorGenerationFailed = -5,
}

type LiteRtLmEnginePtr = *mut std::ffi::c_void;
type LiteRtLmConversationPtr = *mut std::ffi::c_void;

// Benchmark FFI types
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct LiteRtLmTurnBenchmark {
    num_tokens: u64,
    duration_seconds: f64,
    tokens_per_sec: f64,
}

#[repr(C)]
struct LiteRtLmBenchmarkInfoFFI {
    total_prefill_turns: u32,
    prefill_turns: *mut LiteRtLmTurnBenchmark,
    total_decode_turns: u32,
    decode_turns: *mut LiteRtLmTurnBenchmark,
    time_to_first_token_ms: f64,
    last_prefill_token_count: u64,
    last_decode_token_count: u64,
}

#[cfg(litert_dynamic)]
#[link(name = "litert_lm_rust_api")]
extern "C" {
    fn LiteRtLmEngine_Create(
        model_path: *const c_char,
        backend: LiteRtLmBackendFFI,
        out_engine: *mut LiteRtLmEnginePtr,
    ) -> c_int;

    fn LiteRtLmEngine_Destroy(engine: LiteRtLmEnginePtr);

    fn LiteRtLmConversation_Create(
        engine: LiteRtLmEnginePtr,
        out_conversation: *mut LiteRtLmConversationPtr,
    ) -> c_int;

    fn LiteRtLmConversation_CreateWithSystem(
        engine: LiteRtLmEnginePtr,
        system_instruction: *const c_char,
        out_conversation: *mut LiteRtLmConversationPtr,
    ) -> c_int;

    fn LiteRtLmConversation_SendMessage(
        conversation: LiteRtLmConversationPtr,
        role: *const c_char,
        content: *const c_char,
        out_response: *mut *mut c_char,
    ) -> c_int;

    fn LiteRtLmConversation_Destroy(conversation: LiteRtLmConversationPtr);

    fn LiteRtLm_FreeString(s: *mut c_char);

    fn LiteRtLm_GetLastError() -> *const c_char;

    fn LiteRtLmConversation_GetBenchmarkInfo(
        conversation: LiteRtLmConversationPtr,
        out_benchmark: *mut *mut LiteRtLmBenchmarkInfoFFI,
    ) -> c_int;

    fn LiteRtLm_FreeBenchmark(benchmark: *mut LiteRtLmBenchmarkInfoFFI);
}

// ============================================================================
// Public API
// ============================================================================

/// Backend type for model execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteRTBackend {
    /// CPU backend
    Cpu,
    /// GPU backend (if available)
    Gpu,
}

impl LiteRTBackend {
    fn to_ffi(self) -> LiteRtLmBackendFFI {
        match self {
            LiteRTBackend::Cpu => LiteRtLmBackendFFI::Cpu,
            LiteRTBackend::Gpu => LiteRtLmBackendFFI::Gpu,
        }
    }
}

/// Benchmark data for a single turn (prefill or decode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnBenchmark {
    pub num_tokens: u64,
    pub duration_seconds: f64,
    pub tokens_per_sec: f64,
}

/// Complete benchmark information for a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkInfo {
    /// All prefill turns
    pub prefill_turns: Vec<TurnBenchmark>,

    /// All decode turns
    pub decode_turns: Vec<TurnBenchmark>,

    /// Time to first token in milliseconds
    pub time_to_first_token_ms: f64,

    /// Convenience: last prefill token count
    pub last_prefill_token_count: u64,

    /// Convenience: last decode token count
    pub last_decode_token_count: u64,
}

impl BenchmarkInfo {
    /// Get average prefill tokens per second across all turns
    pub fn avg_prefill_tokens_per_sec(&self) -> f64 {
        if self.prefill_turns.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.prefill_turns.iter().map(|t| t.tokens_per_sec).sum();
        sum / self.prefill_turns.len() as f64
    }

    /// Get average decode tokens per second across all turns
    pub fn avg_decode_tokens_per_sec(&self) -> f64 {
        if self.decode_turns.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.decode_turns.iter().map(|t| t.tokens_per_sec).sum();
        sum / self.decode_turns.len() as f64
    }

    /// Get total tokens processed (prefill + decode)
    pub fn total_tokens(&self) -> u64 {
        let prefill: u64 = self.prefill_turns.iter().map(|t| t.num_tokens).sum();
        let decode: u64 = self.decode_turns.iter().map(|t| t.num_tokens).sum();
        prefill + decode
    }

    /// Get total duration in seconds (prefill + decode)
    pub fn total_duration_seconds(&self) -> f64 {
        let prefill: f64 = self.prefill_turns.iter().map(|t| t.duration_seconds).sum();
        let decode: f64 = self.decode_turns.iter().map(|t| t.duration_seconds).sum();
        prefill + decode
    }

    /// Get overall tokens per second (total tokens / total duration)
    pub fn overall_tokens_per_sec(&self) -> f64 {
        let total_duration = self.total_duration_seconds();
        if total_duration == 0.0 {
            return 0.0;
        }
        self.total_tokens() as f64 / total_duration
    }

    #[cfg(litert_dynamic)]
    unsafe fn from_ffi(ffi: *const LiteRtLmBenchmarkInfoFFI) -> LlmResult<Self> {
        if ffi.is_null() {
            return Err(LlmError::RuntimeError("Null benchmark info pointer".into()));
        }

        let info = &*ffi;

        // Convert prefill turns
        let prefill_turns: Vec<TurnBenchmark> = if info.total_prefill_turns > 0 {
            std::slice::from_raw_parts(info.prefill_turns, info.total_prefill_turns as usize)
                .iter()
                .map(|t| TurnBenchmark {
                    num_tokens: t.num_tokens,
                    duration_seconds: t.duration_seconds,
                    tokens_per_sec: t.tokens_per_sec,
                })
                .collect()
        } else {
            Vec::new()
        };

        // Convert decode turns
        let decode_turns: Vec<TurnBenchmark> = if info.total_decode_turns > 0 {
            std::slice::from_raw_parts(info.decode_turns, info.total_decode_turns as usize)
                .iter()
                .map(|t| TurnBenchmark {
                    num_tokens: t.num_tokens,
                    duration_seconds: t.duration_seconds,
                    tokens_per_sec: t.tokens_per_sec,
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(BenchmarkInfo {
            prefill_turns,
            decode_turns,
            time_to_first_token_ms: info.time_to_first_token_ms,
            last_prefill_token_count: info.last_prefill_token_count,
            last_decode_token_count: info.last_decode_token_count,
        })
    }

    #[cfg(litert_stub)]
    fn stub() -> Self {
        BenchmarkInfo {
            prefill_turns: Vec::new(),
            decode_turns: Vec::new(),
            time_to_first_token_ms: 0.0,
            last_prefill_token_count: 0,
            last_decode_token_count: 0,
        }
    }
}

/// LiteRT-LM Engine
///
/// The Engine loads a model and manages its lifecycle.
/// Create sessions (conversations) from the engine.
pub struct LiteRTEngine {
    #[cfg(litert_dynamic)]
    ptr: LiteRtLmEnginePtr,
}

// Safety: Engine can be sent between threads
unsafe impl Send for LiteRTEngine {}
unsafe impl Sync for LiteRTEngine {}

impl LiteRTEngine {
    /// Create a new Engine from a model file
    ///
    /// # Arguments
    ///
    /// * `model_path` - Path to the .litertlm model file
    /// * `backend` - Backend to use (Cpu or Gpu)
    pub fn new(model_path: &str, backend: LiteRTBackend) -> LlmResult<Self> {
        #[cfg(litert_dynamic)]
        {
            let model_path_cstr = CString::new(model_path)
                .map_err(|e| LlmError::BindingError(format!("Invalid model path: {}", e)))?;

            let mut engine_ptr: LiteRtLmEnginePtr = std::ptr::null_mut();

            let status = unsafe {
                LiteRtLmEngine_Create(model_path_cstr.as_ptr(), backend.to_ffi(), &mut engine_ptr)
            };

            if status != 0 || engine_ptr.is_null() {
                let err = unsafe {
                    let err_ptr = LiteRtLm_GetLastError();
                    if err_ptr.is_null() {
                        "Unknown error".to_string()
                    } else {
                        CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                    }
                };
                return Err(LlmError::BindingError(err));
            }

            Ok(LiteRTEngine { ptr: engine_ptr })
        }

        #[cfg(litert_stub)]
        {
            let _ = (model_path, backend);
            Err(LlmError::BindingError(
                "LiteRT runtime is not available on this build; set LITERT_LM_PATH to enable it."
                    .to_string(),
            ))
        }
    }

    /// Create a new conversation
    ///
    /// Conversations maintain conversation state and can generate responses.
    pub fn create_conversation(&self) -> LlmResult<LiteRTConversation> {
        self.create_conversation_with_system(None)
    }

    /// Create a new conversation with system instruction
    ///
    /// # Arguments
    ///
    /// * `system_instruction` - Optional system instruction to set conversation context
    pub fn create_conversation_with_system(
        &self,
        system_instruction: Option<&str>,
    ) -> LlmResult<LiteRTConversation> {
        #[cfg(litert_dynamic)]
        {
            let mut conversation_ptr: LiteRtLmConversationPtr = std::ptr::null_mut();

            let status = if let Some(system) = system_instruction {
                let system_cstr = CString::new(system).map_err(|e| {
                    LlmError::BindingError(format!("Invalid system instruction: {}", e))
                })?;
                unsafe {
                    LiteRtLmConversation_CreateWithSystem(
                        self.ptr,
                        system_cstr.as_ptr(),
                        &mut conversation_ptr,
                    )
                }
            } else {
                unsafe { LiteRtLmConversation_Create(self.ptr, &mut conversation_ptr) }
            };

            if status != 0 || conversation_ptr.is_null() {
                let err = unsafe {
                    let err_ptr = LiteRtLm_GetLastError();
                    if err_ptr.is_null() {
                        "Unknown error".to_string()
                    } else {
                        CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                    }
                };
                return Err(LlmError::BindingError(err));
            }

            Ok(LiteRTConversation {
                ptr: conversation_ptr,
            })
        }

        #[cfg(litert_stub)]
        {
            let _ = system_instruction;
            Err(LlmError::BindingError(
                "LiteRT runtime is not available on this build; set LITERT_LM_PATH to enable it."
                    .to_string(),
            ))
        }
    }

    /// Create a new session (conversation) - DEPRECATED, use create_conversation instead
    ///
    /// Sessions maintain conversation state and can generate responses.
    #[deprecated(since = "0.1.0", note = "Use create_conversation instead")]
    pub fn create_session(&self) -> LlmResult<LiteRTConversation> {
        self.create_conversation()
    }
}

#[cfg(litert_dynamic)]
impl Drop for LiteRTEngine {
    fn drop(&mut self) {
        unsafe {
            LiteRtLmEngine_Destroy(self.ptr);
        }
    }
}

#[cfg(litert_stub)]
impl Drop for LiteRTEngine {
    fn drop(&mut self) {}
}

/// LiteRT-LM Conversation
///
/// A conversation represents a stateful chat context with the model.
/// Uses LiteRT-LM's Conversation API with proper Jinja template formatting.
pub struct LiteRTConversation {
    #[cfg(litert_dynamic)]
    ptr: LiteRtLmConversationPtr,
}

// Safety: Conversations can be moved between threads and shared
// The underlying C++ conversation is thread-safe
unsafe impl Send for LiteRTConversation {}
unsafe impl Sync for LiteRTConversation {}

impl LiteRTConversation {
    /// Send a message with a specific role and get a response
    ///
    /// # Arguments
    ///
    /// * `role` - Message role ("user", "model", "system")
    /// * `content` - Message content
    ///
    /// # Returns
    ///
    /// The model's response text
    pub fn send_message(&self, role: &str, content: &str) -> LlmResult<String> {
        #[cfg(litert_dynamic)]
        {
            let role_cstr = CString::new(role)
                .map_err(|e| LlmError::BindingError(format!("Invalid role: {}", e)))?;
            let content_cstr = CString::new(content)
                .map_err(|e| LlmError::BindingError(format!("Invalid content: {}", e)))?;

            let mut response_ptr: *mut c_char = std::ptr::null_mut();

            let status = unsafe {
                LiteRtLmConversation_SendMessage(
                    self.ptr,
                    role_cstr.as_ptr(),
                    content_cstr.as_ptr(),
                    &mut response_ptr,
                )
            };

            if status != 0 || response_ptr.is_null() {
                let err = unsafe {
                    let err_ptr = LiteRtLm_GetLastError();
                    if err_ptr.is_null() {
                        "Unknown error".to_string()
                    } else {
                        CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                    }
                };
                return Err(LlmError::BindingError(err));
            }

            let response = unsafe {
                let response_cstr = CStr::from_ptr(response_ptr);
                let response_string = response_cstr.to_string_lossy().into_owned();
                LiteRtLm_FreeString(response_ptr);
                response_string
            };

            Ok(response)
        }

        #[cfg(litert_stub)]
        {
            let _ = (role, content);
            Err(LlmError::BindingError(
                "LiteRT runtime is not available on this build; set LITERT_LM_PATH to enable it."
                    .to_string(),
            ))
        }
    }

    /// Send a user message and get a response (convenience method)
    ///
    /// # Arguments
    ///
    /// * `content` - User message content
    ///
    /// # Returns
    ///
    /// The model's response text
    pub fn send_user_message(&self, content: &str) -> LlmResult<String> {
        self.send_message("user", content)
    }

    /// Generate a text response to a prompt - DEPRECATED, use send_user_message instead
    ///
    /// # Arguments
    ///
    /// * `prompt` - The input text prompt
    ///
    /// # Returns
    ///
    /// The generated text response
    #[deprecated(since = "0.1.0", note = "Use send_user_message instead")]
    pub fn generate(&self, prompt: &str) -> LlmResult<String> {
        self.send_user_message(prompt)
    }

    /// Get benchmark information for this conversation.
    ///
    /// Returns an error if benchmarking is not enabled in the engine config.
    ///
    /// # Returns
    ///
    /// Benchmark data including prefill/decode turns, TTFT, and tokens/sec
    pub fn get_benchmark_info(&self) -> LlmResult<BenchmarkInfo> {
        #[cfg(litert_dynamic)]
        {
            let mut benchmark_ptr: *mut LiteRtLmBenchmarkInfoFFI = std::ptr::null_mut();

            let status =
                unsafe { LiteRtLmConversation_GetBenchmarkInfo(self.ptr, &mut benchmark_ptr) };

            if status != 0 {
                let err = unsafe {
                    let err_ptr = LiteRtLm_GetLastError();
                    if err_ptr.is_null() {
                        "Failed to get benchmark info".to_string()
                    } else {
                        CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                    }
                };
                return Err(LlmError::BindingError(err));
            }

            let result = unsafe { BenchmarkInfo::from_ffi(benchmark_ptr) };

            // Free the C structure
            unsafe {
                LiteRtLm_FreeBenchmark(benchmark_ptr);
            }

            result
        }

        #[cfg(litert_stub)]
        {
            Ok(BenchmarkInfo::stub())
        }
    }
}

#[cfg(litert_dynamic)]
impl Drop for LiteRTConversation {
    fn drop(&mut self) {
        unsafe {
            LiteRtLmConversation_Destroy(self.ptr);
        }
    }
}

#[cfg(litert_stub)]
impl Drop for LiteRTConversation {
    fn drop(&mut self) {}
}

// Type alias for backward compatibility
pub type LiteRTSession = LiteRTConversation;

/// Tool definition for LiteRT-LM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Structured LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Response format type
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResponseFormat {
    Text,
    Json,
}

// Type alias for backward compatibility - LiteRTSession is already defined as alias to LiteRTConversation at line 356

/// Response metadata for structured outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub format: ResponseFormat,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_types() {
        let cpu = LiteRTBackend::Cpu;
        let gpu = LiteRTBackend::Gpu;
        assert!(matches!(cpu, LiteRTBackend::Cpu));
        assert!(matches!(gpu, LiteRTBackend::Gpu));
    }
}
