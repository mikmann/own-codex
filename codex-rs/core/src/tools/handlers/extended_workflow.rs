use crate::extended_workflow::ExpertResult;
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
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SubAgentActivityKind;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::protocol::WorkflowAgentDisplay;
use codex_protocol::protocol::WorkflowAgentDisplayStatus;
use codex_protocol::protocol::WorkflowAgentValidationDisplay;
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
    expected_subject: Option<String>,
    agent_task: Option<String>,
    selected: Option<bool>,
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
            let arguments = match &invocation.payload {
                ToolPayload::Function { arguments } => arguments,
                _ => {
                    return Err(FunctionCallError::RespondToModel(
                        "plan_workflow_reviewers received unsupported payload".to_string(),
                    ));
                }
            };
            let args: PlanReviewersArgs = parse_arguments(arguments)?;
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
            let planning_input = runtime
                .reviewer_planning_input(&args.change_summary)
                .map_err(FunctionCallError::RespondToModel)?;
            let validations = verified_workflow_validations(&invocation).await?;
            match runtime.pre_reviewer_gate(&validations) {
                crate::extended_workflow::WorkflowCompletionGate::Satisfied => {}
                crate::extended_workflow::WorkflowCompletionGate::Continue(message)
                | crate::extended_workflow::WorkflowCompletionGate::Blocked(message) => {
                    return Err(FunctionCallError::RespondToModel(message));
                }
            }
            let change_digest_sha256 = working_tree_change_digest(&invocation).await?;
            let mut plan = plan_reviewers(
                &planning_input,
                &invocation.turn.sub_id,
                &invocation.turn.config.extended_workflow,
            );
            crate::extended_workflow::preflight_process_plan(
                &mut plan,
                &invocation.turn.config,
                &planning_input,
            )
            .await
            .map_err(FunctionCallError::RespondToModel)?;
            crate::tools::handlers::multi_agents_common::preflight_workflow_process_profiles(
                &invocation.session,
                &invocation.turn,
                &plan,
            )
            .await?;
            plan.change_digest_sha256 = Some(change_digest_sha256.clone());
            runtime
                .install_reviewers(&plan, &args.change_summary, &change_digest_sha256)
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

impl CoreToolRuntime for ValidateWorkflowResultHandler {
    fn waits_for_runtime_cancellation(&self) -> bool {
        true
    }
}

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
    if args.selected == Some(true) && args.kind != ResultKind::Ticket {
        return Err(FunctionCallError::RespondToModel(
            "selected=true is valid only for a ticket result".to_string(),
        ));
    }
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
    if let Some(expected_subject) = args.expected_subject.as_deref() {
        crate::extended_workflow::require_bounded_workflow_text(
            "expected_subject",
            expected_subject,
            crate::extended_workflow::MAX_WORKFLOW_EXPECTED_SUBJECT_BYTES,
        )
        .map_err(FunctionCallError::RespondToModel)?;
    }
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
    let (change_digest_sha256, change_digest_error) = if args.kind == ResultKind::Reviewer {
        let current_digest = working_tree_change_digest(&invocation).await?;
        let expected_digest = invocation
            .turn
            .extension_data
            .get::<WorkflowTurnRuntimeState>()
            .and_then(|runtime| runtime.reviewer_change_digest_sha256());
        let error = match expected_digest {
            Some(expected_digest) if expected_digest != current_digest => Some(format!(
                "actual working-tree change digest {current_digest} does not match the reviewer round digest {expected_digest}; plan a new reviewer round for the changed diff"
            )),
            Some(_) => None,
            None => Some(
                "reviewer result is not bound to an installed working-tree change digest"
                    .to_string(),
            ),
        };
        (Some(current_digest), error)
    } else {
        (None, None)
    };
    let key = target.key.clone();
    let (output_revision, output_sha256, requires_higher_revision, agent_thread_id, agent_role) =
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
                    Some(agent.display.role.clone()),
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
            reviewer_role: agent_role,
            output_revision,
            output_sha256: output_sha256.clone(),
            requires_higher_revision,
        })
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    let submission = match submission {
        codex_agent_graph_store::WorkflowValidationDecision::Attempt(submission) => submission,
        codex_agent_graph_store::WorkflowValidationDecision::AlreadyAccepted(submissions)
            if args.kind == ResultKind::Ticket && args.selected == Some(true) =>
        {
            ensure_accepted_validation_matches(&invocation, &key, &output_sha256).await?;
            select_accepted_ticket(&invocation, result_id, &output_sha256).await?;
            return Ok(boxed_tool_output(WorkflowToolOutput::new(
                true,
                serde_json::json!({
                    "accepted": true,
                    "already_accepted": true,
                    "retry_allowed": false,
                    "review_cycle_allowed": false,
                    "attempts": submissions,
                    "errors": [],
                    "has_unresolved_findings": false,
                    "has_high_or_critical": false,
                    "formal_status": "valid",
                    "reviewer_verdict": null,
                    "gate_status": "eligible",
                    "error_code": null,
                    "finding_count": 0,
                    "max_severity": null,
                    "change_digest_sha256": null,
                    "passing_reviewers": null,
                    "required_reviewers": null,
                    "result_bytes": encoded_result.len(),
                    "max_result_bytes": max_result_bytes,
                    "validation_key": key,
                    "selected": true,
                }),
            )));
        }
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
    let reviewer_contract_validation = if args.kind == ResultKind::Reviewer {
        Some(
            match serde_json::from_value::<ReviewerResult>(args.result.clone()) {
                Ok(result) => {
                    let validation = validate_reviewer_result(&result, &target.expected_subject);
                    reviewer_result = Some(result);
                    validation
                }
                Err(error) => invalid_contract("ReviewerResult", error),
            },
        )
    } else {
        None
    };
    let mut validation = if let Some(error) = target
        .agent
        .as_ref()
        .and_then(|agent| agent.terminal_error.as_ref())
    {
        invalid_validation(error.clone())
    } else if let Some(agent) = target.agent.as_ref()
        && agent.terminal_result.as_ref() != Some(&args.result)
    {
        invalid_validation(
                "submitted result does not exactly match the completed workflow agent's terminal JSON response"
                    .to_string(),
        )
    } else if let Some(error) = change_digest_error {
        invalid_validation(error)
    } else if encoded_result.len() > max_result_bytes {
        invalid_validation(format!(
            "structured result is {} bytes; the configured hard limit is {max_result_bytes} bytes",
            encoded_result.len()
        ))
    } else {
        match args.kind {
            ResultKind::Expert => match serde_json::from_value::<ExpertResult>(args.result.clone())
            {
                Ok(result) => validate_expert_result(&result, &target.expected_subject),
                Err(error) => invalid_contract("ExpertResult", error),
            },
            ResultKind::Reviewer => reviewer_contract_validation
                .clone()
                .unwrap_or_else(|| {
                    invalid_validation(
                        "internal workflow validation failure: reviewer contract validation was not initialized"
                            .to_string(),
                    )
                }),
            ResultKind::Ticket => {
                match serde_json::from_value::<WorkflowTicket>(args.result.clone()) {
                    Ok(result) => validate_ticket(&result, &target.expected_subject),
                    Err(error) => invalid_contract("WorkflowTicket", error),
                }
            }
            ResultKind::Concept => {
                match serde_json::from_value::<WorkflowConcept>(args.result.clone()) {
                    Ok(result) => validate_concept(&result, &target.expected_subject),
                    Err(error) => invalid_contract("WorkflowConcept", error),
                }
            }
        }
    };
    if let Some(reviewer_validation) = reviewer_contract_validation.as_ref() {
        validation.has_high_or_critical = reviewer_validation.has_high_or_critical;
        validation.finding_count = reviewer_validation.finding_count;
        validation.max_severity = reviewer_validation.max_severity;
        if validation.errors != reviewer_validation.errors && !reviewer_validation.errors.is_empty()
        {
            validation.errors.extend(reviewer_validation.errors.clone());
        }
    }

    let has_unresolved_findings = reviewer_result
        .as_ref()
        .is_some_and(|result| !result.findings.is_empty());
    let reviewer_verdict = reviewer_result.as_ref().map(|result| result.verdict);
    let reviewer_pass = validation.valid
        && reviewer_result.as_ref().is_some_and(|result| {
            result.verdict == ReviewVerdict::Pass
                && !has_unresolved_findings
                && !validation.has_high_or_critical
        });
    let retry_allowed = !validation.valid && submission == 1;
    let (passing_reviewers, required_reviewers) =
        if let Some(change_digest_sha256) = change_digest_sha256.as_deref() {
            let validations = invocation
                .session
                .services
                .agent_control
                .list_workflow_validations(invocation.session.thread_id, &invocation.turn.sub_id)
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let required = invocation
                .turn
                .extension_data
                .get::<WorkflowTurnRuntimeState>()
                .map_or(0, |runtime| runtime.reviewer_required_count());
            let (passing, required) = reviewer_round_counts(
                &validations,
                &key,
                reviewer_pass,
                change_digest_sha256,
                required,
            );
            (Some(passing), Some(required))
        } else {
            (None, None)
        };
    let presentation =
        crate::tools::handlers::extended_workflow_validation::validation_presentation(
            crate::tools::handlers::extended_workflow_validation::WorkflowValidationPresentationArgs {
                validation: &validation,
                reviewer_verdict,
                submission,
                retry_allowed,
                change_digest_sha256: change_digest_sha256.clone(),
                passing_reviewers,
                required_reviewers,
            },
        );
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
            output_sha256: output_sha256.clone(),
            accepted: validation.valid,
            reviewer_pass,
            has_high_or_critical: validation.has_high_or_critical,
            details: presentation.stored,
        })
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;

    let mut review_cycle_allowed = false;
    if validation.valid {
        if args.kind == ResultKind::Reviewer && !reviewer_pass {
            review_cycle_allowed = invocation
                .session
                .services
                .agent_control
                .list_workflow_validations(invocation.session.thread_id, &invocation.turn.sub_id)
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?
                .iter()
                .find(|validation| validation.validation_key == key)
                .is_some_and(|validation| validation.review_cycles < 2);
        }
        if args.kind == ResultKind::Ticket && args.selected == Some(true) {
            select_accepted_ticket(&invocation, result_id, &output_sha256).await?;
        }
        if let Some(result) = reviewer_result.as_ref() {
            emit_reviewer_findings(&invocation, result).await;
        }
    }

    emit_validation_status(
        &invocation,
        &args,
        &target,
        validation.valid,
        presentation.display.clone(),
    )
    .await;

    Ok(boxed_tool_output(WorkflowToolOutput::new(
        validation.valid,
        serde_json::json!({
            "accepted": validation.valid,
            "retry_allowed": retry_allowed,
            "review_cycle_allowed": review_cycle_allowed,
            "attempts": submission,
            "errors": validation.errors,
            "has_unresolved_findings": has_unresolved_findings,
            "has_high_or_critical": validation.has_high_or_critical,
            "formal_status": presentation.display.formal_status,
            "reviewer_verdict": presentation.display.reviewer_verdict,
            "gate_status": presentation.display.gate_status,
            "error_code": presentation.display.error_code,
            "finding_count": presentation.display.finding_count,
            "max_severity": presentation.display.max_severity,
            "change_digest_sha256": presentation.display.change_digest_sha256,
            "passing_reviewers": presentation.display.passing_reviewers,
            "required_reviewers": presentation.display.required_reviewers,
            "result_bytes": encoded_result.len(),
            "max_result_bytes": max_result_bytes,
            "validation_key": key,
            "selected": args.kind == ResultKind::Ticket && args.selected == Some(true),
        }),
    )))
}

async fn select_accepted_ticket(
    invocation: &ToolInvocation,
    result_id: &str,
    ticket_output_sha256: &str,
) -> Result<(), FunctionCallError> {
    let selection_sha256 = crate::extended_workflow::sha256_bytes(
        format!("selected_ticket\0{result_id}\0{ticket_output_sha256}").as_bytes(),
    );
    let decision = invocation
        .session
        .services
        .agent_control
        .begin_workflow_validation(codex_agent_graph_store::WorkflowValidationSubmission {
            root_thread_id: invocation.session.thread_id,
            turn_id: invocation.turn.sub_id.clone(),
            validation_key: "selected_ticket".to_string(),
            result_kind: "selected_ticket".to_string(),
            agent_thread_id: None,
            reviewer_role: Some(result_id.to_string()),
            output_revision: 1,
            output_sha256: selection_sha256.clone(),
            requires_higher_revision: false,
        })
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    let submission = match decision {
        codex_agent_graph_store::WorkflowValidationDecision::Attempt(submission) => submission,
        codex_agent_graph_store::WorkflowValidationDecision::AlreadyAccepted(_) => {
            let validations = invocation
                .session
                .services
                .agent_control
                .list_workflow_validations(invocation.session.thread_id, &invocation.turn.sub_id)
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let same_selection = validations.iter().any(|record| {
                record.validation_key == "selected_ticket"
                    && record.result_kind == "selected_ticket"
                    && record.accepted
                    && record.agent_role.as_deref() == Some(result_id)
                    && record.output_sha256 == selection_sha256
            });
            if same_selection {
                return Ok(());
            }
            return Err(FunctionCallError::RespondToModel(
                "a different ticket was already selected for this turn and the selection is immutable"
                    .to_string(),
            ));
        }
        codex_agent_graph_store::WorkflowValidationDecision::Exhausted(_)
        | codex_agent_graph_store::WorkflowValidationDecision::UnchangedCorrection(_) => {
            return Err(FunctionCallError::RespondToModel(
                "the ticket selection marker is unusable for this turn".to_string(),
            ));
        }
        codex_agent_graph_store::WorkflowValidationDecision::ResultLimitReached => {
            return Err(FunctionCallError::RespondToModel(
                "the turn contains too many workflow results to persist a ticket selection"
                    .to_string(),
            ));
        }
    };
    invocation
        .session
        .services
        .agent_control
        .complete_workflow_validation(codex_agent_graph_store::WorkflowValidationCompletion {
            root_thread_id: invocation.session.thread_id,
            turn_id: invocation.turn.sub_id.clone(),
            validation_key: "selected_ticket".to_string(),
            submission,
            output_revision: 1,
            output_sha256: selection_sha256,
            accepted: true,
            reviewer_pass: false,
            has_high_or_critical: false,
            details: codex_agent_graph_store::WorkflowValidationDetails {
                gate_status: "eligible".to_string(),
                ..Default::default()
            },
        })
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))
}

async fn ensure_accepted_validation_matches(
    invocation: &ToolInvocation,
    validation_key: &str,
    output_sha256: &str,
) -> Result<(), FunctionCallError> {
    let validations = invocation
        .session
        .services
        .agent_control
        .list_workflow_validations(invocation.session.thread_id, &invocation.turn.sub_id)
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    if validations.iter().any(|record| {
        record.validation_key == validation_key
            && record.accepted
            && record.output_sha256 == output_sha256
    }) {
        Ok(())
    } else {
        Err(FunctionCallError::RespondToModel(format!(
            "accepted workflow result `{validation_key}` does not match this resubmission"
        )))
    }
}

async fn verified_workflow_validations(
    invocation: &ToolInvocation,
) -> Result<Vec<codex_agent_graph_store::WorkflowValidationRecord>, FunctionCallError> {
    let mut validations = invocation
        .session
        .services
        .agent_control
        .list_workflow_validations(invocation.session.thread_id, &invocation.turn.sub_id)
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    for validation in &mut validations {
        let Some(agent_thread_id) = validation.agent_thread_id else {
            continue;
        };
        if !validation.accepted {
            continue;
        }
        let completed = invocation
            .session
            .services
            .agent_control
            .completed_workflow_agent(agent_thread_id)
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        validation.accepted &= completed.output_revision == validation.output_revision
            && completed.output_sha256 == validation.output_sha256;
        validation.reviewer_pass &= validation.accepted;
    }
    Ok(validations)
}

async fn working_tree_change_digest(
    invocation: &ToolInvocation,
) -> Result<String, FunctionCallError> {
    let cwd = invocation
        .turn
        .environments
        .single_local_environment_cwd()
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "reviewer diff binding requires exactly one ready local turn environment"
                    .to_string(),
            )
        })?;
    let diff = codex_git_utils::working_tree_diff(cwd.as_path())
        .await
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "the actual working-tree diff could not be read; reviewer planning and validation fail closed until Git is available in a valid worktree"
                    .to_string(),
            )
        })?;
    Ok(crate::extended_workflow::sha256_bytes(diff.as_bytes()))
}

fn reviewer_round_counts(
    validations: &[codex_agent_graph_store::WorkflowValidationRecord],
    validation_key: &str,
    current_reviewer_pass: bool,
    change_digest_sha256: &str,
    required_reviewers: usize,
) -> (u32, u32) {
    let passing = validations
        .iter()
        .filter(|record| {
            record.validation_key != validation_key
                && record.result_kind == "reviewer"
                && record.accepted
                && record.reviewer_pass
                && record.details.change_digest_sha256.as_deref() == Some(change_digest_sha256)
        })
        .count()
        + usize::from(current_reviewer_pass);
    (
        u32::try_from(passing).unwrap_or(u32::MAX),
        u32::try_from(required_reviewers).unwrap_or(u32::MAX),
    )
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
            let runtime = invocation
                .turn
                .extension_data
                .get::<WorkflowTurnRuntimeState>()
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "ticket and concept validation require an active root workflow turn"
                            .to_string(),
                    )
                })?;
            Ok(ValidationTarget {
                key: format!("{}:{}", args.kind.as_str(), args.result_id.trim()),
                expected_subject: runtime
                    .root_request()
                    .map_err(FunctionCallError::RespondToModel)?,
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
            let workflow_root_sha256 = identity.workflow_root_sha256.clone().ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "agent `{thread_id}` is missing its persisted root-request binding"
                ))
            })?;
            let expected_assignment_sha256 = crate::extended_workflow::workflow_assignment_sha256(
                &invocation.turn.sub_id,
                &workflow_root_sha256,
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
            {
                if runtime.root_request_sha256() != workflow_root_sha256 {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "agent `{thread_id}` is bound to a different root request"
                    )));
                }
                if let Some(assignment) = runtime.assignment_for_thread(thread_id)
                    && (assignment.kind != expected_kind
                        || assignment.name != name
                        || assignment.role != identity.role
                        || assignment.task != assignment_message)
                {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "agent `{thread_id}` does not match the server-side assignment for this turn"
                    )));
                }
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
                        validation: None,
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
    validation: WorkflowAgentValidationDisplay,
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
                    validation: None,
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
    display.validation = Some(validation);
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
            "review_cycle_allowed": false,
            "attempts": submissions,
            "errors": [error],
            "has_unresolved_findings": false,
            "has_high_or_critical": false,
            "formal_status": "invalid",
            "reviewer_verdict": null,
            "gate_status": "excluded",
            "error_code": "correction_exhausted",
            "finding_count": 0,
            "max_severity": null,
            "change_digest_sha256": null,
            "passing_reviewers": null,
            "required_reviewers": null,
            "result_bytes": result_bytes,
            "max_result_bytes": max_result_bytes,
            "validation_key": validation_key,
            "selected": false,
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
        finding_count: 0,
        max_severity: None,
    }
}

fn invalid_validation(error: String) -> crate::extended_workflow::ContractValidation {
    crate::extended_workflow::ContractValidation {
        valid: false,
        errors: vec![error],
        has_high_or_critical: false,
        finding_count: 0,
        max_severity: None,
    }
}

async fn emit_reviewer_findings(invocation: &ToolInvocation, result: &ReviewerResult) {
    for finding in &result.findings {
        let message = format!(
            "[{:?}] {}::{} — {} Recommended correction: {}{}",
            finding.effective_severity(),
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
            .send_event(invocation.turn.as_ref(), reviewer_finding_event(message))
            .await;
    }
}

fn reviewer_finding_event(message: String) -> EventMsg {
    EventMsg::Warning(WarningEvent { message })
}

#[cfg(test)]
#[path = "extended_workflow_tests.rs"]
mod tests;
