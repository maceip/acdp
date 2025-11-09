use std::sync::Arc;

use async_trait::async_trait;
use acdp_common::gencode::{
    CapabilityId, CapabilityKind, CapabilityOrigin, CapabilitySink, CapabilityToken,
    CodeGenerationRequest, CodeGenerationResponse, ExecutionPayload, ExecutionPlan, PlanGraph,
    PlanNode, PlanNodeKind,
};
use acdp_llm::service::{GenerationEvent, GenerationRequest, LlmService};
use serde_json::Value;

use crate::Result;

/// Trait implemented by code-generation backends.
#[async_trait]
pub trait CodeGenerator: Send + Sync {
    /// Produce an execution plan for the given request.
    async fn generate_plan(&self, request: CodeGenerationRequest)
        -> Result<CodeGenerationResponse>;

    /// Record execution outcome for GEPA/telemetry purposes.
    async fn record_execution_outcome(
        &self,
        _plan: &ExecutionPlan,
        _success: bool,
        _metadata: Value,
    ) -> Result<()> {
        Ok(())
    }
}

/// Placeholder generator that returns empty plans.
#[derive(Debug, Default)]
pub struct NullCodeGenerator;

#[async_trait]
impl CodeGenerator for NullCodeGenerator {
    async fn generate_plan(
        &self,
        _request: CodeGenerationRequest,
    ) -> Result<CodeGenerationResponse> {
        Ok(CodeGenerationResponse::empty())
    }
}

/// LLM-backed code generator that produces sandbox-friendly execution plans.
pub struct LlmCodeGenerator {
    llm_service: Arc<LlmService>,
}

impl LlmCodeGenerator {
    pub fn new(llm_service: Arc<LlmService>) -> Self {
        Self { llm_service }
    }
}

#[async_trait]
impl CodeGenerator for LlmCodeGenerator {
    async fn generate_plan(
        &self,
        request: CodeGenerationRequest,
    ) -> Result<CodeGenerationResponse> {
        let prompt = build_prompt(&request);
        let mut handle = self
            .llm_service
            .start_generation(GenerationRequest {
                prompt: prompt.clone(),
                temperature: None,
                max_tokens: None,
            })
            .await?;

        let mut generated = String::new();
        while let Some(event) = handle.next().await {
            match event {
                GenerationEvent::Token(token) => generated.push_str(&token),
                GenerationEvent::Completed(_) => break,
            }
        }

        let code = extract_code_block(&generated)
            .unwrap_or_else(|| generated.trim().to_string())
            .trim()
            .to_string();

        let default_language = request
            .spec
            .target_language
            .clone()
            .unwrap_or_else(|| "python".to_string());

        let mut policy = request.policy.clone();
        let capability_id = CapabilityId("sandbox_exec".to_string());
        if !policy
            .capabilities
            .iter()
            .any(|cap| cap.id == capability_id)
        {
            policy.capabilities.push(CapabilityToken {
                id: capability_id.clone(),
                kind: CapabilityKind::SandboxExecution,
                origin: CapabilityOrigin::Trusted,
                allowed_sinks: vec![CapabilitySink::SandboxExecution],
            });
        }

        let mut plan = ExecutionPlan::empty();
        plan.plan_id = request.request_id;
        plan.payload = ExecutionPayload::Script {
            language: default_language,
            code: code.clone(),
        };
        plan.graph = PlanGraph {
            nodes: vec![PlanNode {
                id: "run_python".to_string(),
                kind: PlanNodeKind::RunPython {
                    code: code.clone(),
                    capability: capability_id.clone(),
                },
            }],
        };
        plan.policy = policy;
        plan.metadata = request.metadata.clone();
        plan.metadata
            .entry("prompt_preview".to_string())
            .or_insert(prompt.chars().take(200).collect());
        plan.metadata
            .entry("generated_code_preview".to_string())
            .or_insert(code.chars().take(200).collect());

        Ok(CodeGenerationResponse::new(plan))
    }

    async fn record_execution_outcome(
        &self,
        plan: &ExecutionPlan,
        success: bool,
        metadata: Value,
    ) -> Result<()> {
        let preview = plan
            .metadata
            .get("prompt_preview")
            .cloned()
            .unwrap_or_else(|| plan.plan_id.to_string());
        self.llm_service
            .record_codegen_attempt(&preview, success, metadata)
            .await?;
        Ok(())
    }
}

fn build_prompt(request: &CodeGenerationRequest) -> String {
    let mut prompt = format!(
        "You are a secure code generation assistant. Generate minimal {} code that satisfies the following description:\n{}\n",
        request
            .spec
            .target_language
            .as_deref()
            .unwrap_or("python"),
        request.spec.description
    );

    if !request.spec.context.is_empty() {
        prompt.push_str("\nContext:\n");
        for (key, value) in &request.spec.context {
            prompt.push_str(&format!(" - {}: {}\n", key, value));
        }
    }

    prompt.push_str("\nReturn ONLY the code, wrapped in a fenced code block.");
    prompt
}

fn extract_code_block(response: &str) -> Option<String> {
    let mut lines = response.lines();
    let mut inside_block = false;
    let mut _language_prefix = None;
    let mut collected = Vec::new();

    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```") {
            if inside_block {
                break;
            } else {
                inside_block = true;
                _language_prefix = Some(line.trim().to_string());
                continue;
            }
        }

        if inside_block {
            collected.push(line);
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected.join("\n"))
    }
}
