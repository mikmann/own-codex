use super::*;
use crate::extended_workflow::WorkflowAgentKind;
use crate::extended_workflow::WorkflowTurnRuntimeState;
use crate::tools::handlers::multi_agents_spec::create_close_agent_tool_v2;
use codex_tools::ToolSpec;

pub(crate) struct Handler;

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("close_agent")
    }

    fn spec(&self) -> ToolSpec {
        create_close_agent_tool_v2()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move { handle_close_agent(invocation).await.map(boxed_tool_output) })
    }
}

async fn handle_close_agent(
    invocation: ToolInvocation,
) -> Result<CloseAgentResult, FunctionCallError> {
    let ToolInvocation {
        session,
        turn,
        payload,
        call_id,
        ..
    } = invocation;
    let arguments = function_arguments(payload)?;
    let args: CloseAgentArgs = parse_arguments(&arguments)?;
    let agent_id = resolve_agent_target(&session, &turn, &args.target).await?;
    if agent_id == session.thread_id {
        return Err(FunctionCallError::RespondToModel(
            "an agent cannot close itself; return your result and let the parent close you if needed"
                .to_string(),
        ));
    }
    ensure_workflow_agent_can_close(session.as_ref(), turn.as_ref(), agent_id).await?;
    session
        .emit_turn_item_started(
            &turn,
            &TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                id: call_id.clone(),
                tool: CollabAgentTool::CloseAgent,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: session.thread_id,
                receiver_thread_ids: vec![agent_id],
                receiver_agents: Vec::new(),
                prompt: None,
                model: None,
                reasoning_effort: None,
                agents_states: Default::default(),
            }),
        )
        .await;

    // The control layer recognizes a persisted thread-spawn child after the first close has
    // removed it from the live registry. Both first and repeated calls use the same cascade-close
    // path so persisted spawn-edge state remains authoritative.
    let close_result = Box::pin(
        session
            .services
            .agent_control
            .close_agent(session.thread_id, agent_id),
    )
    .await;
    let (status, previous_status) = match &close_result {
        Ok(outcome) => (
            CollabAgentToolCallStatus::Completed,
            Some(outcome.previous_status.clone()),
        ),
        Err(_) => (CollabAgentToolCallStatus::Failed, None),
    };
    session
        .emit_turn_item_completed(
            &turn,
            TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                id: call_id,
                tool: CollabAgentTool::CloseAgent,
                status,
                sender_thread_id: session.thread_id,
                receiver_thread_ids: vec![agent_id],
                receiver_agents: Vec::new(),
                prompt: None,
                model: None,
                reasoning_effort: None,
                agents_states: previous_status
                    .clone()
                    .map(|status| [(agent_id, status)].into_iter().collect())
                    .unwrap_or_default(),
            }),
        )
        .await;
    close_result.map_err(|err| close_agent_error(agent_id, err))?;

    Ok(CloseAgentResult {
        previous_status: previous_status.unwrap_or(AgentStatus::NotFound),
    })
}

async fn ensure_workflow_agent_can_close(
    session: &crate::session::session::Session,
    turn: &crate::session::turn_context::TurnContext,
    agent_id: codex_protocol::ThreadId,
) -> Result<(), FunctionCallError> {
    let Some(runtime) = turn.extension_data.get::<WorkflowTurnRuntimeState>() else {
        return Ok(());
    };
    let Some(assignment) = runtime.assignment_for_thread(agent_id) else {
        return Ok(());
    };
    let validation_key = format!(
        "{}:{agent_id}",
        match assignment.kind {
            WorkflowAgentKind::Expert => "expert",
            WorkflowAgentKind::Reviewer => "reviewer",
        }
    );
    let validations = session
        .services
        .agent_control
        .list_workflow_validations(session.thread_id, &turn.sub_id)
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    let validation = validations
        .iter()
        .find(|validation| validation.validation_key == validation_key);
    let terminal_validation = validation.is_some_and(|validation| match assignment.kind {
        WorkflowAgentKind::Expert => validation.accepted || validation.submissions >= 2,
        WorkflowAgentKind::Reviewer => {
            if validation.accepted {
                validation.reviewer_pass || validation.review_cycles >= 2
            } else {
                validation.submissions >= 2
            }
        }
    });
    if terminal_validation {
        return Ok(());
    }

    let status = session.services.agent_control.get_status(agent_id).await;
    let terminal_without_result =
        matches!(status, AgentStatus::Errored(_) | AgentStatus::Interrupted)
            || matches!(status, AgentStatus::Shutdown | AgentStatus::NotFound)
                && session
                    .services
                    .agent_control
                    .completed_workflow_agent(agent_id)
                    .await
                    .is_err();
    if terminal_without_result {
        return Ok(());
    }

    Err(FunctionCallError::RespondToModel(format!(
        "workflow {} agent `{agent_id}` cannot be closed yet; validate its completed result and finish the permitted correction or review cycle first",
        assignment.kind.label(),
    )))
}

impl CoreToolRuntime for Handler {
    fn waits_for_runtime_cancellation(&self) -> bool {
        true
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseAgentArgs {
    target: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CloseAgentResult {
    pub(crate) previous_status: AgentStatus,
}

impl ToolOutput for CloseAgentResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, "close_agent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, Some(true), "close_agent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "close_agent")
    }
}
