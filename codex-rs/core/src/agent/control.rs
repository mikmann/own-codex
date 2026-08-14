use crate::agent::AgentStatus;
use crate::agent::registry::AgentMetadata;
use crate::agent::registry::AgentRegistry;
use crate::agent::role::DEFAULT_ROLE_NAME;
use crate::agent::role::resolve_role_config;
use crate::agent::status::is_final;
use crate::agent_communication::AgentCommunicationContext;
use crate::agent_communication::AgentCommunicationKind;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::config::Config;
use crate::config::RolloutBudgetConfig;
use crate::environment_selection::TurnEnvironmentSnapshot;
use crate::rollout_budget::RolloutBudget;
use crate::session::emit_subagent_session_started;
use crate::session_prefix::format_inter_agent_completion_message;
use crate::session_prefix::format_subagent_context_line;
use crate::session_prefix::format_subagent_notification_message;
use crate::thread_manager::ResumeThreadWithHistoryOptions;
use crate::thread_manager::ThreadManagerState;
use crate::thread_rollout_truncation::truncate_rollout_to_last_n_fork_turns;
use codex_protocol::AgentPath;
use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ResumedHistory;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadSource;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_protocol::user_input::UserInput;
use codex_thread_store::LoadThreadHistoryParams;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::sync::watch;
use tracing::warn;

pub(crate) use self::execution::AgentExecutionGuard;
use self::execution::AgentExecutionLimiter;
use self::residency::V2Residency;
use self::workflow_results::WorkflowResultCache;

mod execution;
mod legacy;
mod lifecycle;
mod residency;
mod spawn;
mod workflow_results;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpawnAgentForkMode {
    FullHistory,
    LastNTurns(usize),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SpawnAgentOptions {
    pub(crate) fork_parent_spawn_call_id: Option<String>,
    pub(crate) fork_mode: Option<SpawnAgentForkMode>,
    pub(crate) parent_thread_id: Option<ThreadId>,
    pub(crate) parent_turn_id: Option<String>,
    pub(crate) environments: Option<Vec<TurnEnvironmentSelection>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletedWorkflowAgent {
    pub(crate) source: SessionSource,
    pub(crate) task: Option<String>,
    pub(crate) output: String,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
    pub(crate) output_revision: u32,
    pub(crate) output_sha256: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LiveAgent {
    pub(crate) thread_id: ThreadId,
    pub(crate) metadata: AgentMetadata,
    pub(crate) status: AgentStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CloseAgentOutcome {
    pub(crate) previous_status: AgentStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InMemorySpawnEdge {
    parent_thread_id: ThreadId,
    status: codex_agent_graph_store::ThreadSpawnEdgeStatus,
}

#[derive(Clone, Debug)]
struct InMemoryWorkflowValidation {
    submissions: u8,
    accepted: bool,
    result_kind: String,
    agent_thread_id: Option<ThreadId>,
    reviewer_role: Option<String>,
    output_revision: u32,
    output_sha256: String,
    reviewer_pass: bool,
    review_cycles: u8,
    has_high_or_critical: bool,
}

type InMemoryWorkflowValidations = HashMap<(ThreadId, String, String), InMemoryWorkflowValidation>;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ListedAgent {
    pub(crate) agent_name: String,
    pub(crate) agent_status: AgentStatus,
}

/// Control-plane handle for multi-agent operations.
/// `AgentControl` is held by each session (via `SessionServices`). It provides capability to
/// spawn new agents and the inter-agent communication layer.
/// An `AgentControl` instance is intended to be created at most once per root thread/session
/// tree. That same `AgentControl` is then shared with every sub-agent spawned from that root,
/// which keeps the registry scoped to that root thread rather than the entire `ThreadManager`.
#[derive(Clone, Default)]
pub(crate) struct AgentControl {
    /// ID shared by the whole agent control session. This means every sub-agents from a common
    /// root share the same session ID.
    session_id: SessionId,
    /// Weak handle back to the global thread registry/state.
    /// This is `Weak` to avoid reference cycles and shadow persistence of the form
    /// `ThreadManagerState -> CodexThread -> Session -> SessionServices -> ThreadManagerState`.
    manager: Weak<ThreadManagerState>,
    state: Arc<AgentRegistry>,
    v2_residency: Arc<V2Residency>,
    agent_execution_limiter: Arc<AgentExecutionLimiter>,
    /// Session-scoped state shared by the root thread and every cloned sub-agent control handle.
    rollout_budget: Arc<RolloutBudget>,
    /// Serializes lifecycle transitions against spawn publication while still allowing unrelated
    /// spawns in the same agent tree to proceed in parallel.
    lifecycle_gate: Arc<RwLock<()>>,
    /// Allows a failed or crash-recovered close to be retried without overlapping another close.
    close_gate: Arc<AsyncMutex<()>>,
    /// Session-local mirror that also covers ephemeral spawn edges omitted from SQLite.
    spawn_edges: Arc<std::sync::Mutex<HashMap<ThreadId, InMemorySpawnEdge>>>,
    /// Ephemeral fallback for tests and non-persistent thread stores. Local resumable sessions use
    /// the SQLite ledger exposed through `AgentGraphStore` instead.
    workflow_validations: Arc<std::sync::Mutex<InMemoryWorkflowValidations>>,
    /// Bounded to currently open workflow agents and used only when an ephemeral child has no
    /// persisted rollout from which its server-owned terminal result can be recovered.
    workflow_results: Arc<WorkflowResultCache>,
}

impl AgentControl {
    /// Construct a new `AgentControl` that can spawn/message agents via the given manager state.
    pub(crate) fn new(
        manager: Weak<ThreadManagerState>,
        rollout_budget: Option<RolloutBudgetConfig>,
    ) -> Self {
        let control = Self {
            manager,
            ..Default::default()
        };
        if let Some(rollout_budget) = rollout_budget {
            control.rollout_budget.configure(rollout_budget);
        }
        control
    }

    pub(crate) fn with_session_id(mut self, session_id: SessionId, max_threads: usize) -> Self {
        self.session_id = session_id;
        self.agent_execution_limiter.initialize(max_threads);
        self
    }

    pub(crate) fn session_id(&self) -> SessionId {
        self.session_id
    }

    async fn root_thread_id_for(&self, thread_id: ThreadId) -> ThreadId {
        if let Some(root_thread_id) = self.state.agent_id_for_path(&AgentPath::root()) {
            return root_thread_id;
        }
        if let Some(root_thread_id) = self.in_memory_spawn_root(thread_id) {
            return root_thread_id;
        }
        let Ok(state) = self.upgrade() else {
            return ThreadId::from(self.session_id());
        };
        let mut current = thread_id;
        let mut visited = HashSet::new();
        while visited.insert(current) {
            let Ok(thread) = state.get_thread(current).await else {
                break;
            };
            let Some(parent_thread_id) = thread.session_source.parent_thread_id() else {
                return current;
            };
            current = parent_thread_id;
        }
        ThreadId::from(self.session_id())
    }

    pub(crate) fn rollout_budget(&self) -> &RolloutBudget {
        self.rollout_budget.as_ref()
    }

    pub(crate) async fn begin_workflow_validation(
        &self,
        submission: codex_agent_graph_store::WorkflowValidationSubmission,
    ) -> CodexResult<codex_agent_graph_store::WorkflowValidationDecision> {
        let state = self.upgrade()?;
        if let Some(store) = state.agent_graph_store() {
            return store
                .begin_workflow_validation(submission)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to reserve persisted workflow validation: {error}"
                    ))
                });
        }

        let key = (
            submission.root_thread_id,
            submission.turn_id.clone(),
            submission.validation_key.clone(),
        );
        let mut ledger = self
            .workflow_validations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = ledger.get_mut(&key) {
            if existing.accepted
                && existing.result_kind == "reviewer"
                && submission.output_revision > existing.output_revision
            {
                if existing.review_cycles >= 2 {
                    return Ok(codex_agent_graph_store::WorkflowValidationDecision::Exhausted(2));
                }
                existing.submissions = 1;
                existing.accepted = false;
                existing.result_kind = submission.result_kind;
                existing.agent_thread_id = submission.agent_thread_id;
                existing.reviewer_role = submission.reviewer_role;
                existing.output_revision = submission.output_revision;
                existing.output_sha256 = submission.output_sha256;
                existing.reviewer_pass = false;
                existing.has_high_or_critical = false;
                return Ok(codex_agent_graph_store::WorkflowValidationDecision::Attempt(1));
            }
            if existing.accepted
                && existing.result_kind == "reviewer"
                && !existing.reviewer_pass
                && existing.review_cycles >= 2
            {
                return Ok(codex_agent_graph_store::WorkflowValidationDecision::Exhausted(2));
            }
            if existing.accepted && (existing.result_kind != "reviewer" || existing.reviewer_pass) {
                return Ok(
                    codex_agent_graph_store::WorkflowValidationDecision::AlreadyAccepted(
                        existing.submissions,
                    ),
                );
            }
            if existing.accepted && submission.output_revision <= existing.output_revision {
                return Ok(
                    codex_agent_graph_store::WorkflowValidationDecision::UnchangedCorrection(
                        existing.submissions,
                    ),
                );
            }
            if existing.accepted {
                existing.submissions = 1;
                existing.accepted = false;
                existing.result_kind = submission.result_kind;
                existing.agent_thread_id = submission.agent_thread_id;
                existing.reviewer_role = submission.reviewer_role;
                existing.output_revision = submission.output_revision;
                existing.output_sha256 = submission.output_sha256;
                existing.reviewer_pass = false;
                existing.has_high_or_critical = false;
                return Ok(codex_agent_graph_store::WorkflowValidationDecision::Attempt(1));
            }
            if existing.submissions >= 2 {
                return Ok(
                    codex_agent_graph_store::WorkflowValidationDecision::Exhausted(
                        existing.submissions,
                    ),
                );
            }
            if (submission.requires_higher_revision
                && submission.output_revision <= existing.output_revision)
                || (!submission.requires_higher_revision
                    && submission.output_sha256 == existing.output_sha256)
            {
                existing.submissions = 2;
                return Ok(
                    codex_agent_graph_store::WorkflowValidationDecision::UnchangedCorrection(2),
                );
            }
            existing.submissions += 1;
            existing.result_kind = submission.result_kind;
            existing.agent_thread_id = submission.agent_thread_id;
            existing.reviewer_role = submission.reviewer_role;
            existing.output_revision = submission.output_revision;
            existing.output_sha256 = submission.output_sha256;
            return Ok(
                codex_agent_graph_store::WorkflowValidationDecision::Attempt(existing.submissions),
            );
        }
        let turn_result_count = ledger
            .keys()
            .filter(|(root_thread_id, turn_id, _)| {
                *root_thread_id == submission.root_thread_id && *turn_id == submission.turn_id
            })
            .count();
        if turn_result_count >= 16 {
            return Ok(codex_agent_graph_store::WorkflowValidationDecision::ResultLimitReached);
        }
        ledger.insert(
            key,
            InMemoryWorkflowValidation {
                submissions: 1,
                accepted: false,
                result_kind: submission.result_kind,
                agent_thread_id: submission.agent_thread_id,
                reviewer_role: submission.reviewer_role,
                output_revision: submission.output_revision,
                output_sha256: submission.output_sha256,
                reviewer_pass: false,
                review_cycles: 0,
                has_high_or_critical: false,
            },
        );
        Ok(codex_agent_graph_store::WorkflowValidationDecision::Attempt(1))
    }

    pub(crate) async fn complete_workflow_validation(
        &self,
        completion: codex_agent_graph_store::WorkflowValidationCompletion,
    ) -> CodexResult<()> {
        let state = self.upgrade()?;
        if let Some(store) = state.agent_graph_store() {
            let completed = store
                .complete_workflow_validation(completion)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to persist accepted workflow validation: {error}"
                    ))
                })?;
            if !completed {
                return Err(CodexErr::Fatal(
                    "workflow validation reservation changed before acceptance".to_string(),
                ));
            }
            return Ok(());
        }

        let key = (
            completion.root_thread_id,
            completion.turn_id,
            completion.validation_key,
        );
        let mut ledger = self
            .workflow_validations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(existing) = ledger.get_mut(&key) else {
            return Err(CodexErr::Fatal(
                "workflow validation reservation is missing".to_string(),
            ));
        };
        if existing.accepted
            || existing.submissions != completion.submission
            || existing.output_revision != completion.output_revision
            || existing.output_sha256 != completion.output_sha256
        {
            return Err(CodexErr::Fatal(
                "workflow validation reservation changed before acceptance".to_string(),
            ));
        }
        existing.accepted = true;
        existing.reviewer_pass = completion.reviewer_pass;
        if existing.result_kind == "reviewer" {
            existing.review_cycles = existing.review_cycles.saturating_add(1).min(2);
        }
        existing.has_high_or_critical = completion.has_high_or_critical;
        Ok(())
    }

    pub(crate) async fn list_workflow_validations(
        &self,
        root_thread_id: ThreadId,
        turn_id: &str,
    ) -> CodexResult<Vec<codex_agent_graph_store::WorkflowValidationRecord>> {
        let state = self.upgrade()?;
        if let Some(store) = state.agent_graph_store() {
            return store
                .list_workflow_validations(root_thread_id, turn_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to read persisted workflow validations: {error}"
                    ))
                });
        }
        let ledger = self
            .workflow_validations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut records = ledger
            .iter()
            .filter(
                |&((record_root, record_turn, _validation_key), _validation)| {
                    *record_root == root_thread_id && record_turn == turn_id
                },
            )
            .map(
                |((_record_root, _record_turn, validation_key), validation)| {
                    codex_agent_graph_store::WorkflowValidationRecord {
                        validation_key: validation_key.clone(),
                        result_kind: validation.result_kind.clone(),
                        agent_thread_id: validation.agent_thread_id,
                        agent_role: validation.reviewer_role.clone(),
                        submissions: validation.submissions,
                        accepted: validation.accepted,
                        review_cycles: validation.review_cycles,
                        output_revision: validation.output_revision,
                        output_sha256: validation.output_sha256.clone(),
                        reviewer_pass: validation.reviewer_pass,
                        has_high_or_critical: validation.has_high_or_critical,
                    }
                },
            )
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.validation_key.cmp(&right.validation_key));
        Ok(records)
    }

    async fn ensure_agent_lifecycle_open(&self, agent_id: ThreadId) -> CodexResult<()> {
        let root_thread_id = self.root_thread_id_for(agent_id).await;
        if agent_id == root_thread_id {
            return Ok(());
        }
        let state = self.upgrade()?;
        let lifecycle_open = if let Some(agent_graph_store) = state.agent_graph_store() {
            if agent_graph_store
                .thread_spawn_edge_status(agent_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to read thread-spawn lifecycle for {agent_id}: {error}"
                    ))
                })?
                .is_some()
            {
                agent_graph_store
                    .thread_spawn_path_is_open(agent_id)
                    .await
                    .map_err(|error| {
                        CodexErr::Fatal(format!(
                            "failed to read thread-spawn ancestor lifecycle for {agent_id}: {error}"
                        ))
                    })?
            } else {
                match self.in_memory_spawn_root(agent_id) {
                    Some(_) => self.in_memory_spawn_path_is_open(root_thread_id, agent_id),
                    None => match state.get_thread(agent_id).await {
                        Ok(thread) => !thread.session_source.is_non_root_agent(),
                        Err(_) => true,
                    },
                }
            }
        } else {
            match self.in_memory_spawn_root(agent_id) {
                Some(_) => self.in_memory_spawn_path_is_open(root_thread_id, agent_id),
                None => match state.get_thread(agent_id).await {
                    Ok(thread) => !thread.session_source.is_non_root_agent(),
                    Err(_) => true,
                },
            }
        };
        if !lifecycle_open {
            return Err(CodexErr::UnsupportedOperation(format!(
                "agent `{agent_id}` is closing or closed and cannot start or resume descendants"
            )));
        }
        Ok(())
    }

    /// Send rich user input items to an existing agent thread.
    pub(crate) async fn send_input(
        &self,
        agent_id: ThreadId,
        input: Vec<UserInput>,
        parent_turn_id: Option<String>,
    ) -> CodexResult<String> {
        let _lifecycle_guard = self.acquire_agent_operation(agent_id).await?;
        let state = self.upgrade()?;
        self.ensure_execution_capacity_for_turn_start(agent_id, /*starts_turn*/ true)
            .await?;
        self.send_input_after_capacity_check(agent_id, &state, input, parent_turn_id)
            .await
    }

    async fn send_input_after_capacity_check(
        &self,
        agent_id: ThreadId,
        state: &Arc<ThreadManagerState>,
        input: Vec<UserInput>,
        parent_turn_id: Option<String>,
    ) -> CodexResult<String> {
        self.handle_thread_request_result(
            agent_id,
            state,
            state.send_op(agent_id, input.into(), parent_turn_id).await,
        )
        .await
    }

    pub(crate) async fn send_inter_agent_communication(
        &self,
        agent_id: ThreadId,
        communication: InterAgentCommunication,
        agent_communication_context: AgentCommunicationContext,
        parent_turn_id: Option<String>,
    ) -> CodexResult<String> {
        let _lifecycle_guard = self.acquire_agent_operation(agent_id).await?;
        let state = self.upgrade()?;
        self.ensure_execution_capacity_for_turn_start(agent_id, communication.trigger_turn)
            .await?;
        self.send_inter_agent_communication_after_capacity_check(
            agent_id,
            &state,
            communication,
            agent_communication_context,
            parent_turn_id,
        )
        .await
    }

    async fn send_inter_agent_communication_after_capacity_check(
        &self,
        agent_id: ThreadId,
        state: &Arc<ThreadManagerState>,
        communication: InterAgentCommunication,
        context: AgentCommunicationContext,
        parent_turn_id: Option<String>,
    ) -> CodexResult<String> {
        self.submit_inter_agent_communication(
            agent_id,
            state,
            communication,
            context,
            parent_turn_id,
        )
        .await
    }

    async fn submit_inter_agent_communication(
        &self,
        agent_id: ThreadId,
        state: &Arc<ThreadManagerState>,
        communication: InterAgentCommunication,
        context: AgentCommunicationContext,
        parent_turn_id: Option<String>,
    ) -> CodexResult<String> {
        let communication_for_log =
            crate::agent_communication::logging_enabled().then(|| communication.clone());
        let parent_turn_id = parent_turn_id.filter(|_| communication.trigger_turn);
        let result = self
            .handle_thread_request_result(
                agent_id,
                state,
                state
                    .send_op(
                        agent_id,
                        Op::InterAgentCommunication { communication },
                        parent_turn_id,
                    )
                    .await,
            )
            .await;
        if let (Some(communication), Ok(communication_id)) =
            (communication_for_log, result.as_ref())
        {
            crate::agent_communication::emit_agent_communication_send(
                communication_id,
                &context,
                &communication,
                agent_id,
            );
        }
        result
    }

    /// Interrupt the current task for an existing agent thread.
    pub(crate) async fn interrupt_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let _lifecycle_guard = self.acquire_agent_operation(agent_id).await?;
        let state = self.upgrade()?;
        self.handle_thread_request_result(
            agent_id,
            &state,
            state
                .send_op(agent_id, Op::Interrupt, /*parent_turn_id*/ None)
                .await,
        )
        .await
    }

    async fn handle_thread_request_result(
        &self,
        agent_id: ThreadId,
        state: &Arc<ThreadManagerState>,
        result: CodexResult<String>,
    ) -> CodexResult<String> {
        if result
            .as_ref()
            .is_err_and(|err| matches!(err.details(), CodexErrorDetails::InternalAgentDied))
        {
            let _ = state.remove_thread(&agent_id).await;
            self.forget_v2_residency(agent_id);
            self.state.release_spawned_thread(agent_id);
        }
        result
    }

    /// Fetch the last known status for `agent_id`, returning `NotFound` when unavailable.
    pub(crate) async fn get_status(&self, agent_id: ThreadId) -> AgentStatus {
        let Ok(state) = self.upgrade() else {
            // No agent available if upgrade fails.
            return AgentStatus::NotFound;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return AgentStatus::NotFound;
        };
        thread.agent_status().await
    }

    pub(crate) fn register_session_root(
        &self,
        current_thread_id: ThreadId,
        current_parent_thread_id: Option<ThreadId>,
    ) {
        if current_parent_thread_id.is_none() {
            self.state.register_root_thread(current_thread_id);
        }
    }

    pub(crate) fn get_agent_metadata(&self, agent_id: ThreadId) -> Option<AgentMetadata> {
        self.state.agent_metadata_for_thread(agent_id)
    }

    pub(crate) fn ensure_agent_known(&self, agent_id: ThreadId) -> CodexResult<AgentMetadata> {
        self.state
            .agent_metadata_for_thread(agent_id)
            .ok_or_else(|| CodexErr::ThreadNotFound(agent_id))
    }

    pub(crate) async fn list_live_agent_subtree_thread_ids(
        &self,
        agent_id: ThreadId,
    ) -> CodexResult<Vec<ThreadId>> {
        let mut thread_ids = vec![agent_id];
        thread_ids.extend(self.live_thread_spawn_descendants(agent_id).await?);
        Ok(thread_ids)
    }

    pub(crate) async fn get_agent_config_snapshot(
        &self,
        agent_id: ThreadId,
    ) -> Option<ThreadConfigSnapshot> {
        let Ok(state) = self.upgrade() else {
            return None;
        };
        let Ok(thread) = state.get_thread(agent_id).await else {
            return None;
        };
        Some(thread.config_snapshot().await)
    }

    pub(crate) async fn resolve_agent_reference(
        &self,
        _current_thread_id: ThreadId,
        current_session_source: &SessionSource,
        agent_reference: &str,
    ) -> CodexResult<ThreadId> {
        let current_agent_path = current_session_source
            .get_agent_path()
            .unwrap_or_else(AgentPath::root);
        let agent_path = current_agent_path
            .resolve(agent_reference)
            .map_err(CodexErr::UnsupportedOperation)?;
        if let Some(thread_id) = self.state.agent_id_for_path(&agent_path) {
            return Ok(thread_id);
        }
        Err(CodexErr::UnsupportedOperation(format!(
            "live agent path `{}` not found",
            agent_path.as_str()
        )))
    }

    /// Subscribe to status updates for `agent_id`, yielding the latest value and changes.
    pub(crate) async fn subscribe_status(
        &self,
        agent_id: ThreadId,
    ) -> CodexResult<watch::Receiver<AgentStatus>> {
        let state = self.upgrade()?;
        let thread = state.get_thread(agent_id).await?;
        Ok(thread.subscribe_status())
    }

    pub(crate) async fn format_environment_context_subagents(
        &self,
        parent_thread_id: ThreadId,
    ) -> String {
        let Ok(agents) = self.open_thread_spawn_children(parent_thread_id).await else {
            return String::new();
        };

        agents
            .into_iter()
            .map(|(thread_id, metadata)| {
                let reference = metadata
                    .agent_path
                    .as_ref()
                    .map(|agent_path| agent_path.name().to_string())
                    .unwrap_or_else(|| thread_id.to_string());
                format_subagent_context_line(reference.as_str(), metadata.agent_nickname.as_deref())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) async fn list_agents(
        &self,
        current_session_source: &SessionSource,
        path_prefix: Option<&str>,
    ) -> CodexResult<Vec<ListedAgent>> {
        let state = self.upgrade()?;
        let resolved_prefix = path_prefix
            .map(|prefix| {
                current_session_source
                    .get_agent_path()
                    .unwrap_or_else(AgentPath::root)
                    .resolve(prefix)
                    .map_err(CodexErr::UnsupportedOperation)
            })
            .transpose()?;

        let mut live_agents = self.state.live_agents();
        live_agents.sort_by(|left, right| {
            left.agent_path
                .as_deref()
                .unwrap_or_default()
                .cmp(right.agent_path.as_deref().unwrap_or_default())
                .then_with(|| {
                    left.agent_id
                        .map(|id| id.to_string())
                        .unwrap_or_default()
                        .cmp(&right.agent_id.map(|id| id.to_string()).unwrap_or_default())
                })
        });

        let root_path = AgentPath::root();
        let mut agents = Vec::with_capacity(live_agents.len().saturating_add(1));
        if resolved_prefix
            .as_ref()
            .is_none_or(|prefix| agent_matches_prefix(Some(&root_path), prefix))
            && let Some(root_thread_id) = self.state.agent_id_for_path(&root_path)
            && let Ok(root_thread) = state.get_thread(root_thread_id).await
        {
            agents.push(ListedAgent {
                agent_name: root_path.to_string(),
                agent_status: root_thread.agent_status().await,
            });
        }

        for metadata in live_agents {
            let Some(thread_id) = metadata.agent_id else {
                continue;
            };
            if resolved_prefix
                .as_ref()
                .is_some_and(|prefix| !agent_matches_prefix(metadata.agent_path.as_ref(), prefix))
            {
                continue;
            }

            let Ok(thread) = state.get_thread(thread_id).await else {
                continue;
            };
            let agent_name = metadata
                .agent_path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| thread_id.to_string());
            agents.push(ListedAgent {
                agent_name,
                agent_status: thread.agent_status().await,
            });
        }

        Ok(agents)
    }

    /// Starts a detached watcher for sub-agents spawned from another thread.
    ///
    /// This is only enabled for `SubAgentSource::ThreadSpawn`, where a parent thread exists and
    /// can receive completion notifications.
    fn maybe_start_completion_watcher(
        &self,
        child_thread_id: ThreadId,
        session_source: Option<SessionSource>,
        child_reference: String,
        child_agent_path: Option<AgentPath>,
    ) {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return;
        };
        let control = self.clone();
        tokio::spawn(async move {
            let status = match control.subscribe_status(child_thread_id).await {
                Ok(mut status_rx) => {
                    let mut status = status_rx.borrow().clone();
                    while !is_final(&status) {
                        if status_rx.changed().await.is_err() {
                            status = control.get_status(child_thread_id).await;
                            break;
                        }
                        status = status_rx.borrow().clone();
                    }
                    status
                }
                Err(_) => control.get_status(child_thread_id).await,
            };
            if !is_final(&status) {
                return;
            }

            let Ok(state) = control.upgrade() else {
                return;
            };
            let child_thread = state.get_thread(child_thread_id).await.ok();
            let child_uses_multi_agent_v2 = match child_thread.as_ref() {
                Some(child_thread) => {
                    child_thread.multi_agent_version() == Some(MultiAgentVersion::V2)
                }
                None => true,
            };
            if child_agent_path.is_some() && child_uses_multi_agent_v2 {
                let Some(child_agent_path) = child_agent_path.clone() else {
                    return;
                };
                let Some(parent_agent_path) = child_agent_path
                    .as_str()
                    .rsplit_once('/')
                    .and_then(|(parent, _)| AgentPath::try_from(parent).ok())
                else {
                    return;
                };
                let Some(message) = format_inter_agent_completion_message(
                    parent_agent_path.clone(),
                    child_agent_path.clone(),
                    &status,
                ) else {
                    return;
                };
                let communication = InterAgentCommunication::new(
                    child_agent_path,
                    parent_agent_path,
                    Vec::new(),
                    message,
                    /*trigger_turn*/ false,
                );
                let context =
                    AgentCommunicationContext::new(AgentCommunicationKind::Result, child_thread_id);
                let _ = control
                    .send_inter_agent_communication(
                        parent_thread_id,
                        communication,
                        context,
                        /*parent_turn_id*/ None,
                    )
                    .await;
                return;
            }
            let message = format_subagent_notification_message(child_reference.as_str(), &status);
            let Ok(parent_thread) = state.get_thread(parent_thread_id).await else {
                return;
            };
            parent_thread
                .inject_user_message_without_turn(message)
                .await;
        });
    }

    fn prepare_agent_metadata(
        &self,
        reservation: &mut crate::agent::registry::SpawnReservation,
        config: &Config,
        agent_path: Option<AgentPath>,
        agent_role: Option<String>,
        preferred_agent_nickname: Option<String>,
    ) -> CodexResult<AgentMetadata> {
        if let Some(agent_path) = agent_path.as_ref() {
            reservation.reserve_agent_path(agent_path)?;
        }
        let candidate_names = spawn::agent_nickname_candidates(config, agent_role.as_deref());
        let candidate_name_refs: Vec<&str> = candidate_names.iter().map(String::as_str).collect();
        let agent_nickname = Some(reservation.reserve_agent_nickname_with_preference(
            &candidate_name_refs,
            preferred_agent_nickname.as_deref(),
        )?);
        Ok(AgentMetadata {
            agent_id: None,
            agent_path,
            agent_nickname,
            agent_role,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_thread_spawn(
        &self,
        reservation: &mut crate::agent::registry::SpawnReservation,
        config: &Config,
        parent_thread_id: ThreadId,
        depth: i32,
        agent_path: Option<AgentPath>,
        agent_role: Option<String>,
        preferred_agent_nickname: Option<String>,
    ) -> CodexResult<(SessionSource, AgentMetadata)> {
        if depth == 1 {
            self.state.register_root_thread(parent_thread_id);
        }
        let source_role = agent_role;
        let display_role = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id,
            depth,
            agent_path: agent_path.clone(),
            agent_nickname: preferred_agent_nickname.clone(),
            agent_role: source_role.clone(),
        })
        .get_agent_display_role();
        let agent_metadata = self.prepare_agent_metadata(
            reservation,
            config,
            agent_path,
            display_role,
            preferred_agent_nickname,
        )?;
        let session_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id,
            depth,
            agent_path: agent_metadata.agent_path.clone(),
            agent_nickname: agent_metadata.agent_nickname.clone(),
            // Keep the source-owned encoded role intact. AgentMetadata contains only the
            // user-facing display role and must not become an authorization source.
            agent_role: source_role,
        });
        Ok((session_source, agent_metadata))
    }

    fn upgrade(&self) -> CodexResult<Arc<ThreadManagerState>> {
        self.manager
            .upgrade()
            .ok_or_else(|| CodexErr::UnsupportedOperation("thread manager dropped".to_string()))
    }

    async fn inherited_environments_for_source(
        &self,
        state: &Arc<ThreadManagerState>,
        session_source: Option<&SessionSource>,
    ) -> Option<TurnEnvironmentSnapshot> {
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return None;
        };

        let parent_thread = state.get_thread(*parent_thread_id).await.ok()?;
        Some(
            parent_thread
                .session
                .services
                .turn_environments
                .snapshot()
                .await,
        )
    }

    async fn inherited_exec_policy_for_source(
        &self,
        state: &Arc<ThreadManagerState>,
        session_source: Option<&SessionSource>,
        child_config: &Config,
    ) -> Option<Arc<crate::exec_policy::ExecPolicyManager>> {
        // Workflow experts and reviewers have a monotonic read-only ceiling. Parent allow rules
        // can imply `bypass_sandbox=true`, so inheriting them would silently punch through that
        // ceiling even though the permission profile itself is read-only.
        if session_source.is_some_and(SessionSource::is_read_only_subagent) {
            return Some(Arc::new(crate::exec_policy::ExecPolicyManager::default()));
        }
        let Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id, ..
        })) = session_source
        else {
            return None;
        };

        let parent_thread = state.get_thread(*parent_thread_id).await.ok()?;
        let parent_config = parent_thread.session.get_config().await;
        if !crate::exec_policy::child_uses_parent_exec_policy(&parent_config, child_config) {
            return None;
        }

        Some(Arc::clone(&parent_thread.session.services.exec_policy))
    }

    async fn open_thread_spawn_children(
        &self,
        parent_thread_id: ThreadId,
    ) -> CodexResult<Vec<(ThreadId, AgentMetadata)>> {
        let mut children_by_parent = self.live_thread_spawn_children().await?;
        Ok(children_by_parent
            .remove(&parent_thread_id)
            .unwrap_or_default())
    }

    async fn live_thread_spawn_children(
        &self,
    ) -> CodexResult<HashMap<ThreadId, Vec<(ThreadId, AgentMetadata)>>> {
        let state = self.upgrade()?;
        let mut children_by_parent = HashMap::<ThreadId, Vec<(ThreadId, AgentMetadata)>>::new();

        for (parent_thread_id, child_thread_id) in state.list_live_thread_spawn_edges().await {
            children_by_parent
                .entry(parent_thread_id)
                .or_default()
                .push((
                    child_thread_id,
                    self.state
                        .agent_metadata_for_thread(child_thread_id)
                        .unwrap_or(AgentMetadata {
                            agent_id: Some(child_thread_id),
                            ..Default::default()
                        }),
                ));
        }

        for children in children_by_parent.values_mut() {
            children.sort_by(|left, right| {
                left.1
                    .agent_path
                    .as_deref()
                    .unwrap_or_default()
                    .cmp(right.1.agent_path.as_deref().unwrap_or_default())
                    .then_with(|| left.0.to_string().cmp(&right.0.to_string()))
            });
        }

        Ok(children_by_parent)
    }

    async fn ensure_thread_spawn_edge_open_for_source(
        &self,
        child_thread: &crate::CodexThread,
        child_thread_id: ThreadId,
        session_source: Option<&SessionSource>,
    ) -> CodexResult<()> {
        let Some(parent_thread_id) = session_source.and_then(SessionSource::parent_thread_id)
        else {
            return Ok(());
        };
        let root_thread_id = self.root_thread_id_for(parent_thread_id).await;
        let ephemeral = child_thread.config_snapshot().await.ephemeral;
        let state = self.upgrade()?;
        if !ephemeral
            && let Some(agent_graph_store) = state.agent_graph_store()
            && !agent_graph_store
                .ensure_thread_spawn_edge_open(root_thread_id, parent_thread_id, child_thread_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to persist open thread-spawn edge for {child_thread_id}: {error}"
                    ))
                })?
        {
            return Err(CodexErr::UnsupportedOperation(format!(
                "parent agent `{parent_thread_id}` is closing or child `{child_thread_id}` was already closed"
            )));
        }
        if !self.ensure_in_memory_spawn_edge_open(root_thread_id, parent_thread_id, child_thread_id)
        {
            return Err(CodexErr::UnsupportedOperation(format!(
                "parent agent `{parent_thread_id}` is closing or child `{child_thread_id}` was already closed"
            )));
        }
        Ok(())
    }

    async fn live_thread_spawn_descendants(
        &self,
        root_thread_id: ThreadId,
    ) -> CodexResult<Vec<ThreadId>> {
        let mut children_by_parent = self.live_thread_spawn_children().await?;
        let mut descendants = Vec::new();
        let mut stack = children_by_parent
            .remove(&root_thread_id)
            .unwrap_or_default()
            .into_iter()
            .map(|(child_thread_id, _)| child_thread_id)
            .rev()
            .collect::<Vec<_>>();

        while let Some(thread_id) = stack.pop() {
            descendants.push(thread_id);
            if let Some(children) = children_by_parent.remove(&thread_id) {
                for (child_thread_id, _) in children.into_iter().rev() {
                    stack.push(child_thread_id);
                }
            }
        }

        Ok(descendants)
    }
}

fn agent_matches_prefix(agent_path: Option<&AgentPath>, prefix: &AgentPath) -> bool {
    if prefix.is_root() {
        return true;
    }

    agent_path.is_some_and(|agent_path| {
        agent_path == prefix
            || agent_path
                .as_str()
                .strip_prefix(prefix.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub(crate) fn render_input_preview(input: &[UserInput]) -> String {
    input
        .iter()
        .map(|item| match item {
            UserInput::Text { text, .. } => text.clone(),
            UserInput::Image { .. } => "[image]".to_string(),
            UserInput::LocalImage { path, .. } => {
                format!("[local_image:{}]", path.display())
            }
            UserInput::Audio { .. } => "[audio]".to_string(),
            UserInput::LocalAudio { path } => {
                format!("[local_audio:{}]", path.display())
            }
            UserInput::Skill { name, path, .. } => {
                format!("[skill:${name}]({})", path.display())
            }
            UserInput::Mention { name, path, .. } => format!("[mention:${name}]({path})"),
            _ => "[input]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn thread_spawn_depth(session_source: &SessionSource) -> Option<i32> {
    match session_source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) => Some(*depth),
        _ => None,
    }
}
#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
