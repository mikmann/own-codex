use super::AgentControl;
use super::CompletedWorkflowAgent;
use crate::agent::AgentStatus;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_thread_store::ReadThreadParams;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Debug)]
struct InMemoryWorkflowAgent {
    source: SessionSource,
    task: String,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    terminal_status: Option<AgentStatus>,
    output_revision: u32,
}

#[derive(Debug, Default)]
pub(super) struct WorkflowResultCache {
    entries: Mutex<HashMap<ThreadId, InMemoryWorkflowAgent>>,
}

impl WorkflowResultCache {
    fn register(
        &self,
        agent_id: ThreadId,
        source: SessionSource,
        task: String,
        model: Option<String>,
        reasoning_effort: Option<ReasoningEffort>,
    ) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                agent_id,
                InMemoryWorkflowAgent {
                    source,
                    task,
                    model,
                    reasoning_effort,
                    terminal_status: None,
                    output_revision: 0,
                },
            );
    }

    fn record_terminal(&self, agent_id: ThreadId, status: &AgentStatus) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(agent) = entries.get_mut(&agent_id) else {
            return;
        };
        if matches!(status, AgentStatus::Completed(_)) {
            agent.output_revision = agent.output_revision.saturating_add(1);
        }
        agent.terminal_status = Some(status.clone());
    }

    fn get(&self, agent_id: ThreadId) -> Option<InMemoryWorkflowAgent> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&agent_id)
            .cloned()
    }

    fn remove(&self, agent_id: ThreadId) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&agent_id);
    }

    fn remove_for_turn(&self, workflow_turn_id: &str) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, agent| {
                agent
                    .source
                    .get_extended_workflow_agent_identity()
                    .and_then(|identity| identity.workflow_turn_id)
                    .as_deref()
                    != Some(workflow_turn_id)
            });
    }
}

impl AgentControl {
    pub(super) fn register_in_memory_workflow_agent(
        &self,
        agent_id: ThreadId,
        source: SessionSource,
        task: String,
        model: Option<String>,
        reasoning_effort: Option<ReasoningEffort>,
    ) {
        self.workflow_results
            .register(agent_id, source, task, model, reasoning_effort);
    }

    pub(crate) fn record_in_memory_workflow_agent_terminal(
        &self,
        agent_id: ThreadId,
        status: &AgentStatus,
    ) {
        self.workflow_results.record_terminal(agent_id, status);
    }

    pub(super) fn remove_in_memory_workflow_agent(&self, agent_id: ThreadId) {
        self.workflow_results.remove(agent_id);
    }

    pub(crate) fn clear_in_memory_workflow_agents_for_turn(&self, workflow_turn_id: &str) {
        self.workflow_results.remove_for_turn(workflow_turn_id);
    }

    /// Return the server-owned terminal output for a workflow child.
    ///
    /// Persisted rollouts remain authoritative for resumable sessions. Ephemeral sessions use the
    /// bounded in-memory record registered before the child receives its first task and retained
    /// through the root turn's completion gate.
    pub(crate) async fn completed_workflow_agent(
        &self,
        agent_id: ThreadId,
    ) -> CodexResult<CompletedWorkflowAgent> {
        let state = self.upgrade()?;
        let live_status = match state.get_thread(agent_id).await {
            Ok(thread) => Some(thread.agent_status().await),
            Err(_) => None,
        };
        let cached = self.workflow_results.get(agent_id);
        let stored = state
            .read_stored_thread(ReadThreadParams {
                thread_id: agent_id,
                include_archived: true,
                include_history: true,
            })
            .await;
        let stored = match stored {
            Ok(stored) => stored,
            Err(error) => {
                let Some(cached) = cached.as_ref() else {
                    return Err(error);
                };
                let cached_status = cached
                    .terminal_status
                    .clone()
                    .unwrap_or(AgentStatus::NotFound);
                let status = match live_status {
                    Some(AgentStatus::Shutdown) => cached_status,
                    Some(status) => status,
                    None => cached_status,
                };
                return completed_workflow_agent_from_status(
                    agent_id,
                    cached.source.clone(),
                    Some(cached.task.clone()),
                    cached.model.clone(),
                    cached.reasoning_effort.clone(),
                    status,
                    cached.output_revision,
                );
            }
        };

        let mut task = None;
        let workflow_agent_path = stored.source.get_agent_path().map(String::from);
        let mut persisted_status = AgentStatus::PendingInit;
        let mut output_revision = 0_u32;
        if let Some(history) = stored.history.as_ref() {
            for item in &history.items {
                match item {
                    RolloutItem::InterAgentCommunication(communication)
                        if task.is_none()
                            && workflow_agent_path.as_deref()
                                == Some(communication.recipient.as_str())
                            && !communication.content.trim().is_empty() =>
                    {
                        task = Some(workflow_assignment_payload(
                            communication.author.as_str(),
                            communication.recipient.as_str(),
                            &communication.content,
                        ));
                    }
                    RolloutItem::ResponseItem(ResponseItem::AgentMessage {
                        author,
                        recipient,
                        content,
                        ..
                    }) if task.is_none()
                        && workflow_agent_path.as_deref() == Some(recipient.as_str()) =>
                    {
                        task = plaintext_agent_message_content(content).map(|content| {
                            workflow_assignment_payload(author, recipient, &content)
                        });
                    }
                    RolloutItem::EventMsg(event) => {
                        if let Some(status) = crate::agent::agent_status_from_event(event) {
                            if matches!(status, AgentStatus::Completed(_)) {
                                output_revision = output_revision.saturating_add(1);
                            }
                            if status != AgentStatus::Shutdown
                                || matches!(
                                    persisted_status,
                                    AgentStatus::PendingInit | AgentStatus::Running
                                )
                            {
                                persisted_status = status;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if task.is_none()
            && let Some(cached) = cached.as_ref()
        {
            task = Some(cached.task.clone());
        }
        let status = match live_status {
            Some(AgentStatus::Shutdown) => persisted_status,
            Some(status) => status,
            None => persisted_status,
        };
        completed_workflow_agent_from_status(
            agent_id,
            stored.source,
            task,
            stored.model,
            stored.reasoning_effort,
            status,
            output_revision,
        )
    }
}

pub(super) fn workflow_assignment_payload(author: &str, recipient: &str, content: &str) -> String {
    let prefix =
        format!("Message Type: NEW_TASK\nTask name: {recipient}\nSender: {author}\nPayload:\n");
    content.strip_prefix(&prefix).unwrap_or(content).to_string()
}

fn completed_workflow_agent_from_status(
    agent_id: ThreadId,
    source: SessionSource,
    task: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    status: AgentStatus,
    output_revision: u32,
) -> CodexResult<CompletedWorkflowAgent> {
    if !source.is_read_only_subagent() || source.get_extended_workflow_agent_identity().is_none() {
        return Err(CodexErr::InvalidRequest(format!(
            "agent `{agent_id}` is not a read-only extended-workflow agent"
        )));
    }

    let output = match status {
        AgentStatus::Completed(Some(output)) if !output.trim().is_empty() => output,
        AgentStatus::Completed(None) => {
            return Err(CodexErr::InvalidRequest(format!(
                "workflow agent `{agent_id}` completed without a terminal response"
            )));
        }
        AgentStatus::PendingInit | AgentStatus::Running => {
            return Err(CodexErr::InvalidRequest(format!(
                "workflow agent `{agent_id}` has not completed"
            )));
        }
        AgentStatus::Errored(error) => {
            return Err(CodexErr::InvalidRequest(format!(
                "workflow agent `{agent_id}` failed: {error}"
            )));
        }
        AgentStatus::Interrupted | AgentStatus::Shutdown | AgentStatus::NotFound => {
            return Err(CodexErr::InvalidRequest(format!(
                "workflow agent `{agent_id}` has no valid completed result (status: {status:?})"
            )));
        }
        AgentStatus::Completed(Some(_)) => {
            return Err(CodexErr::InvalidRequest(format!(
                "workflow agent `{agent_id}` completed with an empty terminal response"
            )));
        }
    };
    let output_revision = output_revision.max(1);
    let output_sha256 = crate::extended_workflow::sha256_bytes(output.as_bytes());

    Ok(CompletedWorkflowAgent {
        source,
        task,
        output,
        model,
        reasoning_effort,
        output_revision,
        output_sha256,
    })
}

#[cfg(test)]
#[path = "workflow_results_tests.rs"]
mod tests;
