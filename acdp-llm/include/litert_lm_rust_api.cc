#include "litert_lm_rust_api.h"

#include <memory>
#include <string>
#include <cstring>

#include "runtime/engine/engine.h"
#include "runtime/conversation/conversation.h"
#include "runtime/conversation/io_types.h"
#include "absl/status/statusor.h"

using namespace litert::lm;

// Thread-local error message storage
thread_local std::string g_last_error;

// Helper to set error message
static void SetError(const std::string& error) {
  g_last_error = error;
}

// Helper to convert absl::Status to int
static int StatusToInt(const absl::Status& status) {
  if (status.ok()) {
    return LITERT_LM_OK;
  }
  SetError(std::string(status.message()));
  return LITERT_LM_ERROR;
}

// ============================================================================
// Engine API
// ============================================================================

int LiteRtLmEngine_Create(
    const char* model_path,
    LiteRtLmBackend backend,
    LiteRtLmEnginePtr* out_engine) {

  if (!model_path || !out_engine) {
    SetError("Invalid arguments: model_path or out_engine is null");
    return LITERT_LM_ERROR_INVALID_ARGS;
  }

  try {
    // Create model assets
    auto model_assets = ModelAssets::Create(model_path);
    if (!model_assets.ok()) {
      SetError("Failed to create model assets: " + std::string(model_assets.status().message()));
      return LITERT_LM_ERROR_MODEL_LOAD_FAILED;
    }

    // Determine backend
    Backend litert_backend = (backend == LITERT_LM_BACKEND_GPU)
        ? Backend::GPU
        : Backend::CPU;

    // Create engine settings
    auto engine_settings = EngineSettings::CreateDefault(
        model_assets.value(),
        litert_backend);

    if (!engine_settings.ok()) {
      SetError("Failed to create engine settings: " + std::string(engine_settings.status().message()));
      return LITERT_LM_ERROR_MODEL_LOAD_FAILED;
    }

    // Create engine
    auto engine = Engine::CreateEngine(engine_settings.value());
    if (!engine.ok()) {
      SetError("Failed to create engine: " + std::string(engine.status().message()));
      return LITERT_LM_ERROR_MODEL_LOAD_FAILED;
    }

    // Transfer ownership to caller
    *out_engine = engine.value().release();
    return LITERT_LM_OK;

  } catch (const std::exception& e) {
    SetError(std::string("Exception in LiteRtLmEngine_Create: ") + e.what());
    return LITERT_LM_ERROR;
  }
}

void LiteRtLmEngine_Destroy(LiteRtLmEnginePtr engine) {
  if (engine) {
    delete static_cast<Engine*>(engine);
  }
}

// ============================================================================
// Conversation API
// ============================================================================

int LiteRtLmConversation_Create(
    LiteRtLmEnginePtr engine,
    LiteRtLmConversationPtr* out_conversation) {

  return LiteRtLmConversation_CreateWithSystem(engine, nullptr, out_conversation);
}

int LiteRtLmConversation_CreateWithSystem(
    LiteRtLmEnginePtr engine,
    const char* system_instruction,
    LiteRtLmConversationPtr* out_conversation) {

  if (!engine || !out_conversation) {
    SetError("Invalid arguments: engine or out_conversation is null");
    return LITERT_LM_ERROR_INVALID_ARGS;
  }

  try {
    Engine* eng = static_cast<Engine*>(engine);

    // Create default conversation config
    auto config = ConversationConfig::CreateDefault(*eng);
    if (!config.ok()) {
      SetError("Failed to create conversation config: " + std::string(config.status().message()));
      return LITERT_LM_ERROR;
    }

    // Add system instruction if provided
    if (system_instruction && strlen(system_instruction) > 0) {
      JsonPreface preface;
      preface.messages = {
        JsonMessage{
          {"role", "system"},
          {"content", system_instruction}
        }
      };
      config->preface = preface;
    }

    // Create conversation
    auto conversation = Conversation::Create(*eng, config.value());
    if (!conversation.ok()) {
      SetError("Failed to create conversation: " + std::string(conversation.status().message()));
      return LITERT_LM_ERROR;
    }

    // Transfer ownership to caller
    *out_conversation = conversation.value().release();
    return LITERT_LM_OK;

  } catch (const std::exception& e) {
    SetError(std::string("Exception in LiteRtLmConversation_Create: ") + e.what());
    return LITERT_LM_ERROR;
  }
}

int LiteRtLmConversation_SendMessage(
    LiteRtLmConversationPtr conversation,
    const char* role,
    const char* content,
    char** out_response) {

  if (!conversation || !role || !content || !out_response) {
    SetError("Invalid arguments: conversation, role, content, or out_response is null");
    return LITERT_LM_ERROR_INVALID_ARGS;
  }

  try {
    Conversation* conv = static_cast<Conversation*>(conversation);

    // Build message
    JsonMessage message{
      {"role", role},
      {"content", content}
    };

    // Send message (blocking)
    auto response = conv->SendMessage(message);
    if (!response.ok()) {
      SetError("Failed to send message: " + std::string(response.status().message()));
      return LITERT_LM_ERROR_GENERATION_FAILED;
    }

    // Extract response text
    std::string response_text;
    auto& response_msg = std::get<JsonMessage>(response.value());

    // Handle both string and array content formats
    if (response_msg["content"].is_string()) {
      response_text = response_msg["content"].get<std::string>();
    } else if (response_msg["content"].is_array()) {
      // Concatenate text from all parts
      for (const auto& part : response_msg["content"]) {
        if (part["type"] == "text" && part.contains("text")) {
          response_text += part["text"].get<std::string>();
        }
      }
    } else {
      SetError("Invalid response format: content is neither string nor array");
      return LITERT_LM_ERROR_GENERATION_FAILED;
    }

    // Allocate and copy response string
    *out_response = strdup(response_text.c_str());
    if (!*out_response) {
      SetError("Failed to allocate memory for response");
      return LITERT_LM_ERROR;
    }

    return LITERT_LM_OK;

  } catch (const std::exception& e) {
    SetError(std::string("Exception in LiteRtLmConversation_SendMessage: ") + e.what());
    return LITERT_LM_ERROR_GENERATION_FAILED;
  }
}

void LiteRtLmConversation_Destroy(LiteRtLmConversationPtr conversation) {
  if (conversation) {
    delete static_cast<Conversation*>(conversation);
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

void LiteRtLm_FreeString(char* str) {
  if (str) {
    free(str);
  }
}

const char* LiteRtLm_GetLastError() {
  return g_last_error.c_str();
}
