use super::*;
use crate::agent::control::SpawnAgentForkMode;
use crate::agent::control::SpawnAgentOptions;
use crate::agent::next_thread_spawn_depth;
use crate::agent::role::DEFAULT_ROLE_NAME;
use crate::agent_communication::AgentCommunicationContext;
use crate::agent_communication::AgentCommunicationKind;
use crate::extended_workflow::WorkflowAgentKind;
use crate::extended_workflow::WorkflowAgentPlan;
use crate::extended_workflow::WorkflowTurnRuntimeState;
use crate::tools::handlers::multi_agents_spec::SpawnAgentToolOptions;
use crate::tools::handlers::multi_agents_spec::create_spawn_agent_tool_v2;
use crate::tools::handlers::multi_agents_v2::message_tool::message_content;
use codex_protocol::AgentPath;
use codex_protocol::protocol::WorkflowAgentDisplay;
use codex_protocol::protocol::WorkflowAgentDisplayStatus;
use codex_tools::ToolSpec;

#[derive(Default)]
pub(crate) struct Handler {
    options: SpawnAgentToolOptions,
}

impl Handler {
    pub(crate) fn new(options: SpawnAgentToolOptions) -> Self {
        Self { options }
    }
}

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("spawn_agent")
    }

    fn spec(&self) -> ToolSpec {
        create_spawn_agent_tool_v2(self.options.clone())
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        let reserved_collaboration_schema = self.options.reserved_collaboration_schema;
        Box::pin(async move {
            handle_spawn_agent(invocation, reserved_collaboration_schema)
                .await
                .map(boxed_tool_output)
        })
    }
}

async fn handle_spawn_agent(
    invocation: ToolInvocation,
    reserved_collaboration_schema: bool,
) -> Result<SpawnAgentResult, FunctionCallError> {
    let ToolInvocation {
        session,
        step_context,
        payload,
        call_id,
        source,
        ..
    } = invocation;
    let turn = &step_context.turn;
    let arguments = function_arguments(payload)?;
    let args: SpawnAgentArgs = parse_arguments(&arguments)?;
    if reserved_collaboration_schema && (args.agent_type.is_some() || args.service_tier.is_some()) {
        return Err(FunctionCallError::RespondToModel(
            "the reserved collaboration.spawn_agent contract does not accept agent_type or service_tier; configure a non-reserved tool namespace for those legacy overrides"
                .to_string(),
        ));
    }
    let workflow_runtime = if turn.config.extended_workflow.enabled
        && crate::extended_workflow::is_root_workflow_source(&turn.session_source)
    {
        Some(
            turn.extension_data
                .get::<WorkflowTurnRuntimeState>()
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "the server-side workflow selection ledger is unavailable for this turn"
                            .to_string(),
                    )
                })?,
        )
    } else {
        None
    };
    let workflow_identity = workflow_runtime
        .as_ref()
        .map(|runtime| runtime.spawn_directive_for_task(&args.task_name))
        .transpose()
        .map_err(FunctionCallError::RespondToModel)?
        .map(|directive| WorkflowIdentity {
            kind: directive.kind,
            profile: directive.profile,
            canonical_task: directive.canonical_task,
            root_request_sha256: directive.root_request_sha256,
        });
    if let Some(identity) = workflow_identity.as_ref() {
        identity.validate_requested_overrides(&args)?;
    }
    let fork_mode = if workflow_identity.is_some() {
        match args.fork_turns.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(value) if value.eq_ignore_ascii_case("none") => None,
            Some(_) => {
                return Err(FunctionCallError::RespondToModel(
                    "workflow experts and reviewers must use fork_turns=\"none\" so only the compact task contract is transferred"
                        .to_string(),
                ));
            }
        }
    } else {
        args.fork_mode()?
    };
    let requested_message = message_content(args.message.clone())?;
    if workflow_identity.is_some() {
        crate::extended_workflow::require_bounded_workflow_text(
            "workflow spawn routing message",
            &requested_message,
            crate::extended_workflow::MAX_WORKFLOW_TASK_BYTES,
        )
        .map_err(FunctionCallError::RespondToModel)?;
    }
    let message = workflow_identity
        .as_ref()
        .map_or(requested_message, |identity| {
            identity.canonical_task.clone()
        });
    let role_name = workflow_identity
        .as_ref()
        .and_then(|identity| identity.profile.agent_type.as_deref())
        .or(args.agent_type.as_deref());
    let requested_model = workflow_identity
        .as_ref()
        .and_then(|identity| identity.profile.model.as_deref())
        .or(args.model.as_deref());
    let requested_reasoning_effort = workflow_identity
        .as_ref()
        .and_then(|identity| identity.profile.reasoning_effort.clone())
        .or_else(|| args.reasoning_effort.clone());

    let session_source = turn.session_source.clone();
    let child_depth = next_thread_spawn_depth(&session_source);
    let mut config = build_agent_spawn_config(
        &session.get_base_instructions().await,
        turn.as_ref(),
        step_context.environments.primary(),
    )?;
    let is_full_history_fork = matches!(fork_mode, Some(SpawnAgentForkMode::FullHistory));
    if is_full_history_fork {
        reject_full_fork_agent_type_override(role_name)?;
    }
    if !is_full_history_fork {
        apply_spawn_agent_role(&session, &mut config, role_name).await?;
    }
    apply_requested_spawn_agent_model_overrides(
        &session,
        turn.as_ref(),
        &mut config,
        requested_model,
        requested_reasoning_effort,
    )
    .await?;
    if let Some(context_budget) = workflow_identity
        .as_ref()
        .and_then(|identity| identity.profile.context_budget)
    {
        let context_budget = i64::try_from(context_budget).map_err(|_| {
            FunctionCallError::RespondToModel(format!(
                "workflow profile context_budget must be at most {} tokens",
                i64::MAX
            ))
        })?;
        config.model_context_window = Some(
            config
                .model_context_window
                .map_or(context_budget, |configured| configured.min(context_budget)),
        );
    }
    apply_spawn_agent_service_tier(
        &session,
        &mut config,
        turn.config.service_tier.as_deref(),
        args.service_tier.as_deref(),
    )
    .await?;
    apply_spawn_agent_runtime_overrides(
        &mut config,
        turn.as_ref(),
        step_context.environments.primary(),
    )?;
    if workflow_identity.is_some() {
        apply_read_only_child_ceiling(&mut config)?;
    }

    let assignment_sha256 = workflow_identity.as_ref().map(|identity| {
        crate::extended_workflow::workflow_assignment_sha256(
            &turn.sub_id,
            &identity.root_request_sha256,
            identity.kind,
            &identity.profile.name,
            &identity.profile.role,
            &message,
        )
    });

    let spawn_source = thread_spawn_source(
        session.thread_id,
        &turn.session_source,
        child_depth,
        ThreadSpawnMetadata {
            agent_role: role_name,
            task_name: Some(args.task_name.clone()),
            read_only: workflow_identity.is_some(),
            preferred_nickname: workflow_identity
                .as_ref()
                .map(|identity| identity.profile.name.as_str()),
            workflow_type: workflow_identity
                .as_ref()
                .map(|identity| identity.kind.label()),
            workflow_role: workflow_identity
                .as_ref()
                .map(|identity| identity.profile.role.as_str()),
            workflow_name: workflow_identity
                .as_ref()
                .map(|identity| identity.profile.name.as_str()),
            workflow_turn_id: workflow_identity.as_ref().map(|_| turn.sub_id.as_str()),
            assignment_sha256: assignment_sha256.as_deref(),
            workflow_root_sha256: workflow_identity
                .as_ref()
                .map(|identity| identity.root_request_sha256.as_str()),
            context_budget: workflow_identity
                .as_ref()
                .and_then(|identity| identity.profile.context_budget),
        },
    )?;
    let new_agent_path = spawn_source.get_agent_path().ok_or_else(|| {
        FunctionCallError::RespondToModel(
            "spawned agent is missing a canonical task name".to_string(),
        )
    })?;
    let author = turn
        .session_source
        .get_agent_path()
        .unwrap_or_else(AgentPath::root);
    // Workflow routing replaces the encrypted model argument with the server-owned canonical
    // task. Deliver that replacement as structured plaintext instead of mislabeling it as
    // ciphertext for the child request.
    let communication_source = if workflow_identity.is_some() {
        crate::tools::context::ToolCallSource::DirectPlaintextMessage
    } else {
        source
    };
    let communication = communication_from_tool_message(
        author,
        new_agent_path.clone(),
        message.clone(),
        &communication_source,
        /*trigger_turn*/ true,
    );
    let context = AgentCommunicationContext::new(AgentCommunicationKind::Spawn, session.thread_id);
    if let (Some(runtime), Some(identity)) = (workflow_runtime.as_ref(), workflow_identity.as_ref())
    {
        let reserved = runtime
            .reserve_spawn_for_task(&args.task_name)
            .map_err(FunctionCallError::RespondToModel)?;
        if reserved.kind != identity.kind
            || reserved.profile != identity.profile
            || reserved.canonical_task != identity.canonical_task
            || reserved.root_request_sha256 != identity.root_request_sha256
        {
            runtime.release_spawn(&args.task_name);
            return Err(FunctionCallError::RespondToModel(
                "the server-side workflow selection changed while the spawn was being prepared"
                    .to_string(),
            ));
        }
        if reserved
            .profile
            .model
            .as_deref()
            .is_some_and(|model| config.model.as_deref() != Some(model))
        {
            runtime.release_spawn(&args.task_name);
            return Err(FunctionCallError::RespondToModel(format!(
                "workflow profile `{}` resolved to model {:?}, not the configured model {:?}",
                identity.profile.name, config.model, reserved.profile.model
            )));
        }
        if reserved
            .profile
            .reasoning_effort
            .as_ref()
            .is_some_and(|effort| config.model_reasoning_effort.as_ref() != Some(effort))
        {
            runtime.release_spawn(&args.task_name);
            return Err(FunctionCallError::RespondToModel(format!(
                "workflow profile `{}` resolved to reasoning effort {:?}, not the configured effort {:?}",
                identity.profile.name,
                config.model_reasoning_effort,
                reserved.profile.reasoning_effort
            )));
        }
    }
    let spawned_agent = Box::pin(
        session
            .services
            .agent_control
            .spawn_agent_with_communication(
                config,
                communication,
                context,
                Some(spawn_source),
                SpawnAgentOptions {
                    fork_parent_spawn_call_id: fork_mode.as_ref().map(|_| call_id.clone()),
                    fork_mode,
                    parent_thread_id: Some(session.thread_id),
                    parent_turn_id: Some(turn.sub_id.clone()),
                    environments: Some(step_context.environments.to_selections()),
                },
            ),
    )
    .await;
    let spawned_agent = match spawned_agent {
        Ok(agent) => agent,
        Err(error) => {
            if let (Some(runtime), Some(_)) =
                (workflow_runtime.as_ref(), workflow_identity.as_ref())
            {
                runtime.release_spawn(&args.task_name);
            }
            return Err(collab_spawn_error(error));
        }
    };
    let new_thread_id = spawned_agent.thread_id;
    if let (Some(runtime), Some(identity)) = (workflow_runtime.as_ref(), workflow_identity.as_ref())
    {
        runtime.commit_spawn(
            new_thread_id,
            identity.kind,
            &identity.profile.name,
            &identity.profile.role,
            &message,
        );
    }
    let agent_snapshot = session
        .services
        .agent_control
        .get_agent_config_snapshot(new_thread_id)
        .await;
    let nickname = agent_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.session_source.get_nickname())
        .or(spawned_agent.metadata.agent_nickname);
    emit_sub_agent_activity(
        &session,
        turn,
        SubAgentActivityItem {
            id: call_id,
            agent_thread_id: new_thread_id,
            agent_path: new_agent_path.clone(),
            kind: SubAgentActivityKind::Started,
            workflow_agent: workflow_identity
                .as_ref()
                .map(|identity| WorkflowAgentDisplay {
                    name: identity.profile.name.clone(),
                    agent_type: identity.kind.label().to_string(),
                    role: identity.profile.role.clone(),
                    model: agent_snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.model.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    reasoning_effort: agent_snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.reasoning_effort.clone()),
                    status: WorkflowAgentDisplayStatus::Running,
                    validation: None,
                }),
        },
    )
    .await;
    let role_tag = role_name.unwrap_or(DEFAULT_ROLE_NAME);
    turn.session_telemetry.counter(
        "codex.multi_agent.spawn",
        /*inc*/ 1,
        &[("role", role_tag), ("version", "v2")],
    );
    let task_name = String::from(new_agent_path);

    let hide_agent_metadata = turn.config.multi_agent_v2.hide_spawn_agent_metadata;
    if hide_agent_metadata {
        Ok(SpawnAgentResult::HiddenMetadata { task_name })
    } else {
        Ok(SpawnAgentResult::WithNickname {
            task_name,
            nickname,
        })
    }
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
struct SpawnAgentArgs {
    message: String,
    task_name: String,
    agent_type: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    service_tier: Option<String>,
    fork_turns: Option<String>,
}

struct WorkflowIdentity {
    kind: WorkflowAgentKind,
    profile: WorkflowAgentPlan,
    canonical_task: String,
    root_request_sha256: String,
}

impl WorkflowIdentity {
    fn validate_requested_overrides(&self, args: &SpawnAgentArgs) -> Result<(), FunctionCallError> {
        if args.agent_type.is_some() || args.service_tier.is_some() {
            return Err(FunctionCallError::RespondToModel(format!(
                "workflow profile `{}` fixes role and service tier server-side; omit agent_type and service_tier",
                self.profile.name
            )));
        }
        if args.model.as_deref().is_some() && args.model.as_deref() != self.profile.model.as_deref()
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "workflow profile `{}` fixes model to {:?}, but the spawn request used {:?}",
                self.profile.name, self.profile.model, args.model
            )));
        }
        if args.reasoning_effort.is_some()
            && args.reasoning_effort.as_ref() != self.profile.reasoning_effort.as_ref()
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "workflow profile `{}` fixes reasoning effort to {:?}, but the spawn request used {:?}",
                self.profile.name, self.profile.reasoning_effort, args.reasoning_effort
            )));
        }
        Ok(())
    }
}

fn apply_read_only_child_ceiling(
    config: &mut crate::config::Config,
) -> Result<(), FunctionCallError> {
    config.apply_read_only_subagent_ceiling().map_err(|error| {
        FunctionCallError::RespondToModel(format!(
            "cannot apply the requested read-only child permission ceiling: {error}"
        ))
    })
}

impl SpawnAgentArgs {
    fn fork_mode(&self) -> Result<Option<SpawnAgentForkMode>, FunctionCallError> {
        let fork_turns = self
            .fork_turns
            .as_deref()
            .map(str::trim)
            .filter(|fork_turns| !fork_turns.is_empty())
            .unwrap_or("all");

        if fork_turns.eq_ignore_ascii_case("none") {
            return Ok(None);
        }
        if fork_turns.eq_ignore_ascii_case("all") {
            return Ok(Some(SpawnAgentForkMode::FullHistory));
        }

        let last_n_turns = fork_turns.parse::<usize>().map_err(|_| {
            FunctionCallError::RespondToModel(
                "fork_turns must be `none`, `all`, or a positive integer string".to_string(),
            )
        })?;
        if last_n_turns == 0 {
            return Err(FunctionCallError::RespondToModel(
                "fork_turns must be `none`, `all`, or a positive integer string".to_string(),
            ));
        }

        Ok(Some(SpawnAgentForkMode::LastNTurns(last_n_turns)))
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum SpawnAgentResult {
    WithNickname {
        task_name: String,
        nickname: Option<String>,
    },
    HiddenMetadata {
        task_name: String,
    },
}

impl ToolOutput for SpawnAgentResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, "spawn_agent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, Some(true), "spawn_agent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "spawn_agent")
    }
}
