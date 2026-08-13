use crate::extended_workflow::ExpertResult;
use crate::extended_workflow::FindingSeverity;
use crate::extended_workflow::ReviewVerdict;
use crate::extended_workflow::ReviewerResult;
use crate::extended_workflow::WorkflowAgentKind;
use crate::extended_workflow::WorkflowConcept;
use crate::extended_workflow::WorkflowTicket;
use crate::extended_workflow::WorkflowTurnRuntimeState;
use crate::extended_workflow::plan_reviewers;
use crate::extended_workflow::validate_concept;
use crate::extended_workflow::validate_expert_result;
use crate::extended_workflow::validate_reviewer_result;
use crate::extended_workflow::validate_ticket;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::extended_workflow_spec::create_plan_workflow_reviewers_tool;
use crate::tools::handlers::extended_workflow_spec::create_validate_workflow_result_tool;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::AgentPath;
use codex_protocol::items::SubAgentActivityItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SubAgentActivityKind;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::protocol::WorkflowAgentDisplay;
use codex_protocol::protocol::WorkflowAgentDisplayStatus;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanReviewersArgs {
    change_summary: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ResultKind {
    Expert,
    Reviewer,
    Ticket,
    Concept,
}

impl ResultKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Expert => "expert",
            Self::Reviewer => "reviewer",
            Self::Ticket => "ticket",
            Self::Concept => "concept",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateResultArgs {
    kind: ResultKind,
    result_id: String,
    expected_subject: String,
    agent_task: Option<String>,
    result: JsonValue,
}

#[derive(Debug, Serialize)]
struct WorkflowToolOutput {
    success: bool,
    payload: JsonValue,
}

impl WorkflowToolOutput {
    fn new(success: bool, payload: impl Serialize) -> Self {
        let payload = serde_json::to_value(payload)
            .unwrap_or_else(|error| serde_json::json!({"serialization_error": error.to_string()}));
        Self { success, payload }
    }
}

impl ToolOutput for WorkflowToolOutput {
    fn log_preview(&self) -> String {
        self.payload.to_string()
    }

    fn success_for_logging(&self) -> bool {
        self.success
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        crate::tools::handlers::multi_agents_common::tool_output_response_item(
            call_id,
            payload,
            &self.payload,
            Some(self.success),
            "extended_workflow",
        )
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        self.payload.clone()
    }
}

pub(crate) struct PlanWorkflowReviewersHandler;

impl ToolExecutor<ToolInvocation> for PlanWorkflowReviewersHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("plan_workflow_reviewers")
    }

    fn spec(&self) -> ToolSpec {
        create_plan_workflow_reviewers_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let arguments = match invocation.payload {
                ToolPayload::Function { arguments } => arguments,
                _ => {
                    return Err(FunctionCallError::RespondToModel(
                        "plan_workflow_reviewers received unsupported payload".to_string(),
                    ));
                }
            };
            let args: PlanReviewersArgs = parse_arguments(&arguments)?;
            if args.change_summary.trim().is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "change_summary must not be empty".to_string(),
                ));
            }
            crate::extended_workflow::require_bounded_workflow_text(
                "change_summary",
                &args.change_summary,
                crate::extended_workflow::MAX_WORKFLOW_CHANGE_SUMMARY_BYTES,
            )
            .map_err(FunctionCallError::RespondToModel)?;
            let plan = plan_reviewers(
                &args.change_summary,
                &invocation.turn.sub_id,
                &invocation.turn.config.extended_workflow,
            );
            let runtime = invocation
                .turn
                .extension_data
                .get::<WorkflowTurnRuntimeState>()
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "reviewer planning is available only inside an active root workflow turn"
                            .to_string(),
                    )
                })?;
            runtime
                .install_reviewers(&plan)
                .map_err(FunctionCallError::RespondToModel)?;
            Ok(boxed_tool_output(WorkflowToolOutput::new(true, plan)))
        })
    }
}

impl CoreToolRuntime for PlanWorkflowReviewersHandler {}

pub(crate) struct ValidateWorkflowResultHandler;

impl ToolExecutor<ToolInvocation> for ValidateWorkflowResultHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("validate_workflow_result")
    }

    fn spec(&self) -> ToolSpec {
        create_validate_workflow_result_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move { handle_validate_result(invocation).await })
    }
}

impl CoreToolRuntime for ValidateWorkflowResultHandler {}

async fn handle_validate_result(
    invocation: ToolInvocation,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let arguments = match &invocation.payload {
        ToolPayload::Function { arguments } => arguments,
        _ => {
            return Err(FunctionCallError::RespondToModel(
                "validate_workflow_result received unsupported payload".to_string(),
            ));
        }
    };
    let args: ValidateResultArgs = parse_arguments(arguments)?;
    let result_id = args.result_id.trim();
    if result_id.is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "result_id must not be empty".to_string(),
        ));
    }
    crate::extended_workflow::require_bounded_workflow_text(
        "result_id",
        result_id,
        crate::extended_workflow::MAX_WORKFLOW_RESULT_ID_BYTES,
    )
    .map_err(FunctionCallError::RespondToModel)?;
    if args.expected_subject.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "expected_subject must not be empty".to_string(),
        ));
    }
    crate::extended_workflow::require_bounded_workflow_text(
        "expected_subject",
        &args.expected_subject,
        crate::extended_workflow::MAX_WORKFLOW_EXPECTED_SUBJECT_BYTES,
    )
    .map_err(FunctionCallError::RespondToModel)?;
    if let Some(agent_task) = args.agent_task.as_deref() {
        crate::extended_workflow::require_bounded_workflow_text(
            "agent_task",
            agent_task,
            crate::extended_workflow::MAX_WORKFLOW_AGENT_REFERENCE_BYTES,
        )
        .map_err(FunctionCallError::RespondToModel)?;
    }

    let encoded_result = serde_json::to_vec(&args.result).map_err(|error| {
        FunctionCallError::RespondToModel(format!("failed to measure structured result: {error}"))
    })?;
    let max_result_bytes = invocation.turn.config.extended_workflow.max_result_bytes;
    let target = validation_target(&invocation, &args).await?;
    let key = target.key.clone();
    let (output_revision, output_sha256, requires_higher_revision, agent_thread_id, reviewer_role) =
        target.agent.as_ref().map_or_else(
            || {
                (
                    1,
                    crate::extended_workflow::sha256_bytes(&encoded_result),
                    false,
                    None,
                    None,
                )
            },
            |agent| {
                (
                    agent.output_revision,
                    agent.output_sha256.clone(),
                    true,
                    Some(agent.thread_id),
                    (args.kind == ResultKind::Reviewer).then(|| agent.display.role.clone()),
                )
            },
        );
    let submission = invocation
        .session
        .services
        .agent_control
        .begin_workflow_validation(codex_agent_graph_store::WorkflowValidationSubmission {
            root_thread_id: invocation.session.thread_id,
            turn_id: invocation.turn.sub_id.clone(),
            validation_key: key.clone(),
            result_kind: args.kind.as_str().to_string(),
            agent_thread_id,
            reviewer_role,
            output_revision,
            output_sha256: output_sha256.clone(),
            requires_higher_revision,
        })
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    let submission = match submission {
        codex_agent_graph_store::WorkflowValidationDecision::Attempt(submission) => submission,
        codex_agent_graph_store::WorkflowValidationDecision::AlreadyAccepted(_) => {
            return Err(FunctionCallError::RespondToModel(format!(
                "result `{result_id}` was already accepted for this turn and cannot be submitted again"
            )));
        }
        codex_agent_graph_store::WorkflowValidationDecision::Exhausted(submissions) => {
            return Ok(validation_rejected_output(
                submissions,
                "the initial validation and the single targeted correction attempt were already consumed",
                encoded_result.len(),
                max_result_bytes,
                &key,
            ));
        }
        codex_agent_graph_store::WorkflowValidationDecision::UnchangedCorrection(submissions) => {
            let error = if requires_higher_revision {
                "the targeted correction must complete a new agent output revision before resubmission"
            } else {
                "the targeted correction must change the structured result before resubmission"
            };
            return Ok(validation_rejected_output(
                submissions,
                error,
                encoded_result.len(),
                max_result_bytes,
                &key,
            ));
        }
        codex_agent_graph_store::WorkflowValidationDecision::ResultLimitReached => {
            return Err(FunctionCallError::RespondToModel(
                "this turn already contains the maximum of 16 distinct workflow validation results"
                    .to_string(),
            ));
        }
    };

    let mut reviewer_result = None;
    let validation = if let Some(error) = target
        .agent
        .as_ref()
        .and_then(|agent| agent.terminal_error.as_ref())
    {
        crate::extended_workflow::ContractValidation {
            valid: false,
            errors: vec![error.clone()],
            has_high_or_critical: false,
        }
    } else if let Some(agent) = target.agent.as_ref()
        && agent.terminal_result.as_ref() != Some(&args.result)
    {
        crate::extended_workflow::ContractValidation {
            valid: false,
            errors: vec![
                "submitted result does not exactly match the completed workflow agent's terminal JSON response"
                    .to_string(),
            ],
            has_high_or_critical: false,
        }
    } else if encoded_result.len() > max_result_bytes {
        crate::extended_workflow::ContractValidation {
            valid: false,
            errors: vec![format!(
                "structured result is {} bytes; the configured hard limit is {max_result_bytes} bytes",
                encoded_result.len()
            )],
            has_high_or_critical: false,
        }
    } else {
        match args.kind {
            ResultKind::Expert => match serde_json::from_value::<ExpertResult>(args.result.clone())
            {
                Ok(result) => validate_expert_result(&result, &target.expected_subject),
                Err(error) => invalid_contract("ExpertResult", error),
            },
            ResultKind::Reviewer => {
                match serde_json::from_value::<ReviewerResult>(args.result.clone()) {
                    Ok(result) => {
                        let validation =
                            validate_reviewer_result(&result, &target.expected_subject);
                        reviewer_result = Some(result);
                        validation
                    }
                    Err(error) => invalid_contract("ReviewerResult", error),
                }
            }
            ResultKind::Ticket => {
                match serde_json::from_value::<WorkflowTicket>(args.result.clone()) {
                    Ok(result) => validate_ticket(&result),
                    Err(error) => invalid_contract("WorkflowTicket", error),
                }
            }
            ResultKind::Concept => {
                match serde_json::from_value::<WorkflowConcept>(args.result.clone()) {
                    Ok(result) => validate_concept(&result),
                    Err(error) => invalid_contract("WorkflowConcept", error),
                }
            }
        }
    };

    if validation.valid {
        let reviewer_pass = reviewer_result.as_ref().is_some_and(|result| {
            result.verdict == ReviewVerdict::Pass && !validation.has_high_or_critical
        });
        invocation
            .session
            .services
            .agent_control
            .complete_workflow_validation(codex_agent_graph_store::WorkflowValidationCompletion {
                root_thread_id: invocation.session.thread_id,
                turn_id: invocation.turn.sub_id.clone(),
                validation_key: key.clone(),
                submission,
                output_revision,
                output_sha256,
                reviewer_pass,
                has_high_or_critical: validation.has_high_or_critical,
            })
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        if let Some(result) = reviewer_result.as_ref() {
            emit_reviewer_findings(&invocation, result).await;
        }
    }

    emit_validation_status(&invocation, &args, &target, validation.valid).await;

    let retry_allowed = !validation.valid && submission == 1;
    Ok(boxed_tool_output(WorkflowToolOutput::new(
        validation.valid,
        serde_json::json!({
            "accepted": validation.valid,
            "retry_allowed": retry_allowed,
            "attempts": submission,
            "errors": validation.errors,
            "has_high_or_critical": validation.has_high_or_critical,
            "result_bytes": encoded_result.len(),
            "max_result_bytes": max_result_bytes,
            "validation_key": key,
        }),
    )))
}

struct ValidationTarget {
    key: String,
    expected_subject: String,
    agent: Option<ValidatedAgentTarget>,
}

struct ValidatedAgentTarget {
    terminal_result: Option<JsonValue>,
    terminal_error: Option<String>,
    thread_id: codex_protocol::ThreadId,
    agent_path: AgentPath,
    display: WorkflowAgentDisplay,
    output_revision: u32,
    output_sha256: String,
}

async fn validation_target(
    invocation: &ToolInvocation,
    args: &ValidateResultArgs,
) -> Result<ValidationTarget, FunctionCallError> {
    match args.kind {
        ResultKind::Ticket | ResultKind::Concept => {
            if args.agent_task.is_some() {
                return Err(FunctionCallError::RespondToModel(
                    "agent_task must be omitted for ticket and concept validation".to_string(),
                ));
            }
            Ok(ValidationTarget {
                key: format!("{}:{}", args.kind.as_str(), args.result_id.trim()),
                expected_subject: args.expected_subject.clone(),
                agent: None,
            })
        }
        ResultKind::Expert | ResultKind::Reviewer => {
            let target = args
                .agent_task
                .as_deref()
                .map(str::trim)
                .filter(|target| !target.is_empty())
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "agent_task is required for expert and reviewer validation".to_string(),
                    )
                })?;
            let thread_id = crate::agent::agent_resolver::resolve_agent_target(
                &invocation.session,
                &invocation.turn,
                target,
            )
            .await?;
            let completed = invocation
                .session
                .services
                .agent_control
                .completed_workflow_agent(thread_id)
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let identity = completed
                .source
                .get_extended_workflow_agent_identity()
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(format!(
                        "agent `{thread_id}` is missing persisted workflow identity"
                    ))
                })?;
            let expected_kind = match args.kind {
                ResultKind::Expert => WorkflowAgentKind::Expert,
                ResultKind::Reviewer => WorkflowAgentKind::Reviewer,
                ResultKind::Ticket | ResultKind::Concept => unreachable!(),
            };
            if !identity
                .agent_type
                .eq_ignore_ascii_case(expected_kind.label())
            {
                return Err(FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` is a {}, not a {}",
                    identity.agent_type,
                    expected_kind.label()
                )));
            }
            if identity.workflow_turn_id.as_deref() != Some(invocation.turn.sub_id.as_str()) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` belongs to a different workflow turn"
                )));
            }
            let name = identity.name.clone().ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` is missing its persisted workflow profile name"
                ))
            })?;
            if completed.source.get_nickname().as_deref() != Some(name.as_str()) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` has inconsistent persisted workflow identity"
                )));
            }
            let assignment_message = completed.task.clone().ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` is missing its persisted workflow assignment"
                ))
            })?;
            let expected_assignment_sha256 = crate::extended_workflow::workflow_assignment_sha256(
                &invocation.turn.sub_id,
                expected_kind,
                &name,
                &identity.role,
                &assignment_message,
            );
            if identity.assignment_sha256.as_deref() != Some(expected_assignment_sha256.as_str()) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` has an invalid persisted workflow assignment digest"
                )));
            }
            if let Some(runtime) = invocation
                .turn
                .extension_data
                .get::<WorkflowTurnRuntimeState>()
                && let Some(assignment) = runtime.assignment_for_thread(thread_id)
                && (assignment.kind != expected_kind
                    || assignment.name != name
                    || assignment.role != identity.role
                    || assignment.task != assignment_message)
            {
                return Err(FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` does not match the server-side assignment for this turn"
                )));
            }
            let expected_subject =
                crate::extended_workflow::workflow_assignment_subject(&assignment_message)
                    .to_string();
            let (terminal_result, terminal_error) = match serde_json::from_str::<JsonValue>(
                completed.output.trim(),
            ) {
                Ok(result) if result.is_object() => (Some(result), None),
                Ok(_) => (
                    None,
                    Some(format!(
                        "workflow agent `{thread_id}` terminal response must be one JSON object"
                    )),
                ),
                Err(error) => (
                    None,
                    Some(format!(
                        "workflow agent `{thread_id}` did not return exactly one JSON object: {error}"
                    )),
                ),
            };
            Ok(ValidationTarget {
                key: format!("{}:{thread_id}", args.kind.as_str()),
                expected_subject,
                agent: Some(ValidatedAgentTarget {
                    terminal_result,
                    terminal_error,
                    thread_id,
                    agent_path: completed
                        .source
                        .get_agent_path()
                        .unwrap_or_else(AgentPath::root),
                    display: WorkflowAgentDisplay {
                        name,
                        agent_type: identity.agent_type,
                        role: identity.role,
                        model: completed.model.unwrap_or_else(|| "unknown".to_string()),
                        reasoning_effort: completed.reasoning_effort,
                        status: WorkflowAgentDisplayStatus::Running,
                    },
                    output_revision: completed.output_revision,
                    output_sha256: completed.output_sha256,
                }),
            })
        }
    }
}

async fn emit_validation_status(
    invocation: &ToolInvocation,
    args: &ValidateResultArgs,
    target: &ValidationTarget,
    accepted: bool,
) {
    let status = if accepted {
        WorkflowAgentDisplayStatus::Validated
    } else {
        WorkflowAgentDisplayStatus::Rejected
    };
    let (thread_id, agent_path, mut display) = target.agent.as_ref().map_or_else(
        || {
            (
                invocation.session.thread_id,
                AgentPath::root(),
                WorkflowAgentDisplay {
                    name: args.result_id.trim().to_string(),
                    agent_type: match args.kind {
                        ResultKind::Expert => "Expert",
                        ResultKind::Reviewer => "Reviewer",
                        ResultKind::Ticket => "Ticket",
                        ResultKind::Concept => "Concept",
                    }
                    .to_string(),
                    role: "Structured result".to_string(),
                    model: invocation
                        .turn
                        .config
                        .model
                        .clone()
                        .unwrap_or_else(|| invocation.turn.model_info.slug.clone()),
                    reasoning_effort: invocation.turn.config.model_reasoning_effort.clone(),
                    status,
                },
            )
        },
        |agent| {
            (
                agent.thread_id,
                agent.agent_path.clone(),
                agent.display.clone(),
            )
        },
    );
    display.status = status;
    crate::tools::handlers::multi_agents_v2::emit_sub_agent_activity(
        &invocation.session,
        &invocation.turn,
        SubAgentActivityItem {
            id: format!("{}:workflow-validation", invocation.call_id),
            kind: SubAgentActivityKind::Interacted,
            agent_thread_id: thread_id,
            agent_path,
            workflow_agent: Some(display),
        },
    )
    .await;
}

fn validation_rejected_output(
    submissions: u8,
    error: &str,
    result_bytes: usize,
    max_result_bytes: usize,
    validation_key: &str,
) -> Box<dyn ToolOutput> {
    boxed_tool_output(WorkflowToolOutput::new(
        false,
        serde_json::json!({
            "accepted": false,
            "retry_allowed": false,
            "attempts": submissions,
            "errors": [error],
            "has_high_or_critical": false,
            "result_bytes": result_bytes,
            "max_result_bytes": max_result_bytes,
            "validation_key": validation_key,
        }),
    ))
}

fn invalid_contract(
    contract: &str,
    error: serde_json::Error,
) -> crate::extended_workflow::ContractValidation {
    crate::extended_workflow::ContractValidation {
        valid: false,
        errors: vec![format!("result does not satisfy {contract}: {error}")],
        has_high_or_critical: false,
    }
}

async fn emit_reviewer_findings(invocation: &ToolInvocation, result: &ReviewerResult) {
    for finding in &result.findings {
        let message = format!(
            "[{:?}] {}::{} — {} Recommended correction: {}{}",
            finding.severity,
            finding.file,
            finding.symbol,
            finding.rationale,
            finding.recommended_fix,
            if finding.missing_tests.is_empty() {
                String::new()
            } else {
                format!(" Missing tests: {}", finding.missing_tests.join(", "))
            },
        );
        invocation
            .session
            .send_event(
                invocation.turn.as_ref(),
                reviewer_finding_event(finding.severity, message),
            )
            .await;
    }
}

fn reviewer_finding_event(severity: FindingSeverity, message: String) -> EventMsg {
    if severity == FindingSeverity::Critical {
        EventMsg::Error(ErrorEvent {
            message,
            codex_error_info: None,
        })
    } else {
        EventMsg::Warning(WarningEvent { message })
    }
}

#[cfg(test)]
#[path = "extended_workflow_tests.rs"]
mod tests;
