//! Sandbox service - main entry point

use crate::execution::{ExecutionId, ExecutionState};
use crate::gencode::{CodeGenerator, LlmCodeGenerator, NullCodeGenerator};
use crate::runtime::Runtime;
use crate::types::{ExecutionRequest, ExecutionResult, ExecutionStream};
use crate::Result;
use anyhow::{anyhow, bail};
use acdp_common::gencode::{
    CapabilityId, CapabilitySink, CapabilityToken, CodeGenerationRequest, CodeGenerationResponse,
    CodeGenerationSpec, ExecutionPlan, ExecutionPolicy, NetworkPolicy, PlanNodeKind,
};
use acdp_llm::service::LlmService;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

/// Sandbox execution service with reconnection support
pub struct SandboxService {
    runtime: Arc<dyn Runtime>,
    code_generator: Arc<dyn CodeGenerator>,
    /// Active executions (for reconnection)
    executions: Arc<RwLock<HashMap<ExecutionId, Arc<RwLock<ExecutionState>>>>>,
}

impl SandboxService {
    /// Create a new sandbox service with the given runtime
    pub fn new(runtime: impl Runtime + 'static) -> Self {
        Self::new_with_generator(runtime, NullCodeGenerator::default())
    }

    /// Create a sandbox service with a custom code generator implementation.
    pub fn new_with_generator(
        runtime: impl Runtime + 'static,
        code_generator: impl CodeGenerator + 'static,
    ) -> Self {
        Self {
            runtime: Arc::new(runtime),
            code_generator: Arc::new(code_generator),
            executions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Construct a sandbox service with an LLM-backed code generator.
    pub fn with_llm(runtime: impl Runtime + 'static, llm_service: Arc<LlmService>) -> Self {
        Self::new_with_generator(runtime, LlmCodeGenerator::new(llm_service))
    }

    /// Execute code and return streaming output
    pub async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionStream> {
        let id = ExecutionId::new();
        self.execute_with_id(id, request).await
    }

    /// Execute with a specific ID (for reconnection)
    pub async fn execute_with_id(
        &self,
        id: ExecutionId,
        request: ExecutionRequest,
    ) -> Result<ExecutionStream> {
        tracing::info!(
            execution_id = %id,
            runtime = self.runtime.name(),
            code_len = request.code.len(),
            "Executing code"
        );

        // Create execution state
        let state = Arc::new(RwLock::new(ExecutionState::new(id, request.clone())));
        self.executions.write().await.insert(id, state.clone());

        // Execute
        let stream = self.runtime.execute(request).await?;

        Ok(stream)
    }

    /// Execute an interpreted execution plan.
    pub async fn execute_plan(&self, plan: ExecutionPlan) -> Result<ExecutionStream> {
        self.interpret_plan(plan).await
    }

    /// Generate and immediately execute an execution plan.
    pub async fn plan_and_execute(
        &self,
        request: CodeGenerationRequest,
    ) -> Result<(CodeGenerationResponse, ExecutionStream)> {
        let response = self.generate_plan(request).await?;
        let stream = self.execute_plan(response.plan.clone()).await?;
        Ok((response, stream))
    }

    /// Get execution state by ID (for reconnection)
    pub async fn get_execution(&self, id: ExecutionId) -> Option<ExecutionState> {
        let executions = self.executions.read().await;
        let state_arc = executions.get(&id)?.clone();
        drop(executions);
        let state = state_arc.read().await.clone();
        Some(state)
    }

    /// List all active executions
    pub async fn list_executions(&self) -> Vec<ExecutionState> {
        let executions = self.executions.read().await;
        let mut states = Vec::new();
        for state in executions.values() {
            states.push(state.read().await.clone());
        }
        states
    }

    /// Clean up completed executions
    pub async fn cleanup_completed(&self) {
        let mut executions = self.executions.write().await;
        executions.retain(|_, state| {
            let status = state.try_read().map(|s| s.status.clone());
            if let Ok(status) = status {
                !matches!(
                    status,
                    crate::execution::ExecutionStatus::Completed
                        | crate::execution::ExecutionStatus::Failed
                        | crate::execution::ExecutionStatus::Cancelled
                )
            } else {
                true
            }
        });
    }

    /// Get the runtime name
    pub fn runtime_name(&self) -> &str {
        self.runtime.name()
    }

    /// Access the installed code generator.
    pub fn code_generator(&self) -> Arc<dyn CodeGenerator> {
        Arc::clone(&self.code_generator)
    }

    /// Produce an execution plan using the configured code generator.
    pub async fn generate_plan(
        &self,
        request: CodeGenerationRequest,
    ) -> Result<CodeGenerationResponse> {
        self.code_generator.generate_plan(request).await
    }

    /// Produce an execution plan for a raw code-generation spec.
    pub async fn generate_plan_from_spec(
        &self,
        spec: CodeGenerationSpec,
        policy: ExecutionPolicy,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<CodeGenerationResponse> {
        let mut request = CodeGenerationRequest::new(spec, policy);
        if let Some(meta) = metadata {
            request.metadata = meta;
        }
        self.generate_plan(request).await
    }

    /// Generate and execute a plan derived from a code-generation specification.
    pub async fn plan_and_execute_spec(
        &self,
        spec: CodeGenerationSpec,
        policy: ExecutionPolicy,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<(CodeGenerationResponse, ExecutionStream)> {
        let response = self.generate_plan_from_spec(spec, policy, metadata).await?;
        let stream = self.execute_plan(response.plan.clone()).await?;
        Ok((response, stream))
    }

    async fn interpret_plan(&self, plan: ExecutionPlan) -> Result<ExecutionStream> {
        if plan.graph.nodes.is_empty() {
            self.code_generator
                .record_execution_outcome(
                    &plan,
                    false,
                    json!({ "error": "plan contains no nodes" }),
                )
                .await?;
            bail!("Execution plan contains no nodes");
        }

        if !plan.policy.mounts.is_empty() {
            self.code_generator
                .record_execution_outcome(
                    &plan,
                    false,
                    json!({ "error": "mounts are not supported yet" }),
                )
                .await?;
            bail!("Mounts are not supported yet");
        }

        if plan.policy.network != NetworkPolicy::Disabled {
            self.code_generator
                .record_execution_outcome(
                    &plan,
                    false,
                    json!({ "error": "network access is not supported yet" }),
                )
                .await?;
            bail!("Network access is not supported yet");
        }

        let mut value_store: HashMap<CapabilityId, String> = HashMap::new();
        let mut last_output = None::<CapabilityId>;
        for node in plan.graph.nodes.iter() {
            match &node.kind {
                PlanNodeKind::RunPython { code, capability } => {
                    self.ensure_node_capability(
                        &plan,
                        capability,
                        CapabilitySink::SandboxExecution,
                        &node.id,
                        "capability_check",
                    )
                    .await?;

                    let output_id = capability.clone();
                    let mut request = ExecutionRequest::new(code.clone());
                    if let Some(timeout) = plan.policy.timeout_secs {
                        request = request.with_timeout(timeout);
                    }

                    let stream = self.execute(request).await.map_err(|err| {
                        self.record_plan_failure(
                            &plan,
                            node.id.clone(),
                            "execute_request",
                            &err.to_string(),
                        );
                        err
                    })?;

                    let ExecutionStream {
                        mut stdout,
                        mut stderr,
                        result,
                    } = stream;

                    let mut stdout_buf = Vec::new();
                    let mut stderr_buf = Vec::new();

                    while let Some(chunk) = stdout.recv().await {
                        stdout_buf.extend(chunk);
                    }

                    while let Some(chunk) = stderr.recv().await {
                        stderr_buf.extend(chunk);
                    }

                    match result.await {
                        Ok(exec_result) => {
                            let output = String::from_utf8_lossy(&stdout_buf).to_string();
                            self.record_plan_result(
                                &plan,
                                &node.id,
                                &exec_result,
                                &output,
                                &String::from_utf8_lossy(&stderr_buf),
                            )
                            .await?;

                            if !exec_result.success() {
                                bail!("node '{}' failed: {:?}", node.id, exec_result.error);
                            }

                            store_value(&mut value_store, output_id.clone(), output);
                            last_output = Some(output_id);
                        }
                        Err(err) => {
                            self.record_plan_failure(
                                &plan,
                                node.id.clone(),
                                "result_channel",
                                &err.to_string(),
                            );
                            bail!("node '{}' failed: {}", node.id, err);
                        }
                    }
                }
                PlanNodeKind::ReadFile {
                    path,
                    output_capability,
                } => {
                    self.ensure_node_capability(
                        &plan,
                        output_capability,
                        CapabilitySink::FileRead,
                        &node.id,
                        "capability_check",
                    )
                    .await?;
                    let content = tokio::fs::read_to_string(path).await.map_err(|err| {
                        self.record_plan_failure(
                            &plan,
                            node.id.clone(),
                            "read_file",
                            &err.to_string(),
                        );
                        err
                    })?;
                    store_value(&mut value_store, output_capability.clone(), content);
                    last_output = Some(output_capability.clone());
                }
                PlanNodeKind::WriteFile {
                    path,
                    input_capability,
                } => {
                    self.ensure_node_capability(
                        &plan,
                        input_capability,
                        CapabilitySink::FileWrite,
                        &node.id,
                        "capability_check",
                    )
                    .await?;
                    let data = value_store
                        .get(input_capability)
                        .cloned()
                        .ok_or_else(|| anyhow!("Capability {} has no value", input_capability.0))?
                        .into_bytes();

                    tokio::fs::write(path, data).await.map_err(|err| {
                        self.record_plan_failure(
                            &plan,
                            node.id.clone(),
                            "write_file",
                            &err.to_string(),
                        );
                        err
                    })?;
                }
                PlanNodeKind::Emit { input_capability } => {
                    self.ensure_node_capability(
                        &plan,
                        input_capability,
                        CapabilitySink::Emit,
                        &node.id,
                        "capability_check",
                    )
                    .await?;
                    let data = value_store
                        .get(input_capability)
                        .cloned()
                        .unwrap_or_default();
                    tracing::info!(
                        "Plan {} emitted data from capability {} ({} bytes)",
                        plan.plan_id,
                        input_capability.0,
                        data.len()
                    );
                    last_output = Some(input_capability.clone());
                }
            }
        }

        if last_output.is_none() {
            self.code_generator
                .record_execution_outcome(
                    &plan,
                    false,
                    json!({ "error": "plan produced no executable nodes" }),
                )
                .await?;
            Err(anyhow!("Execution plan produced no executable nodes"))
        } else {
            let final_id = last_output.unwrap();
            let stdout_data = value_store
                .get(&final_id)
                .cloned()
                .unwrap_or_default()
                .into_bytes();

            let (stdout_tx, stdout_rx) = tokio::sync::mpsc::channel(1);
            let (stderr_tx, stderr_rx) = tokio::sync::mpsc::channel(1);
            let (result_tx, result_rx) = oneshot::channel();

            let _ = stdout_tx.send(stdout_data).await;
            drop(stdout_tx);
            drop(stderr_tx);

            let _ = result_tx.send(ExecutionResult {
                exit_code: 0,
                duration_ms: 0,
                timed_out: false,
                error: None,
            });

            Ok(ExecutionStream {
                stdout: stdout_rx,
                stderr: stderr_rx,
                result: result_rx,
            })
        }
    }

    async fn ensure_node_capability(
        &self,
        plan: &ExecutionPlan,
        capability: &CapabilityId,
        sink: CapabilitySink,
        node_id: &str,
        stage: &str,
    ) -> Result<CapabilityToken> {
        ensure_capability(plan, capability, sink.clone())
            .map_err(|err| {
                self.record_plan_failure(plan, node_id.to_string(), stage, &err.to_string());
                err
            })
            .cloned()
    }

    fn record_plan_failure(&self, plan: &ExecutionPlan, node_id: String, stage: &str, error: &str) {
        let generator = self.code_generator.clone();
        let plan = plan.clone();
        let metadata = json!({
            "plan_id": plan.plan_id,
            "node_id": node_id,
            "stage": stage,
            "error": error,
        });
        tokio::spawn(async move {
            let _ = generator
                .record_execution_outcome(&plan, false, metadata)
                .await;
        });
    }

    async fn record_plan_result(
        &self,
        plan: &ExecutionPlan,
        node_id: &str,
        result: &ExecutionResult,
        stdout: &str,
        stderr: &str,
    ) -> Result<()> {
        let metadata = json!({
            "plan_id": plan.plan_id,
            "node_id": node_id,
            "exit_code": result.exit_code,
            "timed_out": result.timed_out,
            "stdout_preview": stdout.chars().take(200).collect::<String>(),
            "stderr_preview": stderr.chars().take(200).collect::<String>(),
            "error": result.error,
        });
        self.code_generator
            .record_execution_outcome(plan, result.success(), metadata)
            .await
    }
}

fn ensure_capability<'a>(
    plan: &'a ExecutionPlan,
    capability_id: &CapabilityId,
    sink: CapabilitySink,
) -> Result<&'a CapabilityToken> {
    let token = plan
        .policy
        .capabilities
        .iter()
        .find(|cap| cap.id == *capability_id)
        .ok_or_else(|| anyhow!("Capability {} missing", capability_id.0))?;

    if !token.allowed_sinks.contains(&sink) {
        bail!("Capability {} does not permit {:?}", capability_id.0, sink);
    }

    Ok(token)
}

fn store_value(map: &mut HashMap<CapabilityId, String>, capability: CapabilityId, value: String) {
    map.insert(capability, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::WasmRuntime;

    #[tokio::test]
    async fn test_service_creation() {
        let service = SandboxService::new(WasmRuntime::new().unwrap());
        assert_eq!(service.runtime_name(), "wasm");
        let plan = service
            .generate_plan(acdp_common::gencode::CodeGenerationRequest::new(
                Default::default(),
                Default::default(),
            ))
            .await
            .unwrap();
        assert_eq!(
            plan.plan.payload,
            acdp_common::gencode::ExecutionPayload::Empty
        );
    }
}
