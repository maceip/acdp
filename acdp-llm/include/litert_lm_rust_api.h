#ifndef LITERT_LM_RUST_API_H_
#define LITERT_LM_RUST_API_H_

#ifdef __cplusplus
extern "C" {
#endif

// Opaque pointers for C API
typedef void* LiteRtLmEnginePtr;
typedef void* LiteRtLmConversationPtr;

// Backend types
typedef enum {
  LITERT_LM_BACKEND_CPU = 0,
  LITERT_LM_BACKEND_GPU = 1,
} LiteRtLmBackend;

// Status codes
typedef enum {
  LITERT_LM_OK = 0,
  LITERT_LM_ERROR = -1,
  LITERT_LM_ERROR_INVALID_ARGS = -2,
  LITERT_LM_ERROR_NOT_INITIALIZED = -3,
  LITERT_LM_ERROR_MODEL_LOAD_FAILED = -4,
  LITERT_LM_ERROR_GENERATION_FAILED = -5,
} LiteRtLmStatus;

// ============================================================================
// Engine API
// ============================================================================

/**
 * Create a new LiteRT-LM Engine.
 *
 * @param model_path Path to the .litertlm model file
 * @param backend Backend to use (CPU or GPU)
 * @param out_engine Output pointer for the created engine
 * @return Status code (0 = success, negative = error)
 */
int LiteRtLmEngine_Create(
    const char* model_path,
    LiteRtLmBackend backend,
    LiteRtLmEnginePtr* out_engine);

/**
 * Destroy an engine and free resources.
 *
 * @param engine Engine to destroy
 */
void LiteRtLmEngine_Destroy(LiteRtLmEnginePtr engine);

// ============================================================================
// Conversation API
// ============================================================================

/**
 * Create a new Conversation with default config.
 *
 * @param engine Engine to use
 * @param out_conversation Output pointer for the created conversation
 * @return Status code (0 = success, negative = error)
 */
int LiteRtLmConversation_Create(
    LiteRtLmEnginePtr engine,
    LiteRtLmConversationPtr* out_conversation);

/**
 * Create a new Conversation with system instruction.
 *
 * @param engine Engine to use
 * @param system_instruction System instruction for the conversation (can be NULL)
 * @param out_conversation Output pointer for the created conversation
 * @return Status code (0 = success, negative = error)
 */
int LiteRtLmConversation_CreateWithSystem(
    LiteRtLmEnginePtr engine,
    const char* system_instruction,
    LiteRtLmConversationPtr* out_conversation);

/**
 * Send a message to the conversation (blocking).
 *
 * @param conversation Conversation instance
 * @param role Message role ("user", "model", "system")
 * @param content Message content (text)
 * @param out_response Output pointer for the response text (must be freed with LiteRtLm_FreeString)
 * @return Status code (0 = success, negative = error)
 */
int LiteRtLmConversation_SendMessage(
    LiteRtLmConversationPtr conversation,
    const char* role,
    const char* content,
    char** out_response);

/**
 * Destroy a conversation and free resources.
 *
 * @param conversation Conversation to destroy
 */
void LiteRtLmConversation_Destroy(LiteRtLmConversationPtr conversation);

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Free a string allocated by the library.
 *
 * @param str String to free
 */
void LiteRtLm_FreeString(char* str);

/**
 * Get the last error message.
 *
 * @return Error message string (do not free)
 */
const char* LiteRtLm_GetLastError();

#ifdef __cplusplus
}
#endif

#endif  // LITERT_LM_RUST_API_H_
