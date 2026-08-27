use super::session::Session;
use super::turn_context::TurnContext;
use crate::context::ContextualUserFragment;
use crate::workflow_token_budget::WorkflowTokenBudgetLease;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::WORKFLOW_TOKEN_BUDGET_EXTENSION_PERCENTS;

pub(super) async fn maybe_record_reminder(
    sess: &Session,
    turn_context: &TurnContext,
    window_id: &str,
) {
    let budget = sess.services.agent_control.rollout_budget();
    let Some(reminder) = budget.pending_reminder(sess.thread_id(), window_id) else {
        return;
    };
    let response_item = ContextualUserFragment::into(crate::context::RolloutBudgetContext {
        remaining_tokens: reminder.remaining_tokens,
    });
    sess.record_conversation_items(turn_context, std::slice::from_ref(&response_item))
        .await;
    budget.mark_reminder_delivered(sess.thread_id(), window_id, reminder);
}

impl Session {
    pub(crate) fn record_rollout_budget_usage(&self, usage: &TokenUsage) -> CodexResult<()> {
        if self
            .services
            .agent_control
            .rollout_budget()
            .record_usage(usage)?
        {
            return Err(CodexErr::SessionBudgetExceeded);
        }
        Ok(())
    }

    pub(crate) async fn start_or_resume_workflow_token_budget(
        &self,
        turn_context: &TurnContext,
    ) -> CodexResult<()> {
        if !turn_context.config.extended_workflow.enabled {
            return Ok(());
        }
        let limit = turn_context
            .config
            .extended_workflow
            .workflow_budget_tokens
            .unwrap_or(0);
        let limit = i64::try_from(limit).map_err(|_| {
            CodexErr::Fatal("workflow token limit exceeds the supported range".to_string())
        })?;
        let root_thread_id = self
            .services
            .agent_control
            .root_thread_id_for(self.thread_id)
            .await;
        if limit > 0 && self.services.state_db.is_none() {
            return Err(CodexErr::Fatal(
                "workflow token limits require the persistent state database".to_string(),
            ));
        }
        let status = self
            .services
            .agent_control
            .workflow_token_budget()
            .start_or_resume(self.services.state_db.as_ref(), root_thread_id, limit)
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!(
                    "failed to initialize workflow token budget: {error}"
                ))
            })?;
        if status.exhausted() {
            return Err(CodexErr::WorkflowTokenBudgetExceeded);
        }
        Ok(())
    }

    pub(crate) async fn workflow_token_budget_lease(
        &self,
        turn_context: &TurnContext,
    ) -> CodexResult<WorkflowTokenBudgetLease> {
        if !turn_context.config.extended_workflow.enabled {
            return Ok(WorkflowTokenBudgetLease::Disabled);
        }
        let limit = turn_context
            .config
            .extended_workflow
            .workflow_budget_tokens
            .unwrap_or(0);
        if limit > 0 && self.services.state_db.is_none() {
            return Err(CodexErr::Fatal(
                "workflow token limits require the persistent state database".to_string(),
            ));
        }
        let root_thread_id = self
            .services
            .agent_control
            .root_thread_id_for(self.thread_id)
            .await;
        let Some((status, lease)) = self
            .services
            .agent_control
            .workflow_token_budget()
            .lease(self.services.state_db.as_ref(), root_thread_id)
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!("failed to read workflow token budget: {error}"))
            })?
        else {
            return Err(CodexErr::Fatal(
                "active workflow token budget is unavailable".to_string(),
            ));
        };
        if !status.active {
            return Err(CodexErr::WorkflowTokenBudgetExceeded);
        }
        if status.exhausted() {
            return Err(CodexErr::WorkflowTokenBudgetExceeded);
        }
        Ok(lease)
    }

    pub(crate) async fn record_workflow_token_usage(
        &self,
        usage: &TokenUsage,
        lease: &WorkflowTokenBudgetLease,
    ) -> CodexResult<()> {
        if matches!(lease, WorkflowTokenBudgetLease::Disabled) {
            return Ok(());
        }
        let root_thread_id = self
            .services
            .agent_control
            .root_thread_id_for(self.thread_id)
            .await;
        let status = self
            .services
            .agent_control
            .workflow_token_budget()
            .record_usage(
                self.services.state_db.as_ref(),
                root_thread_id,
                lease,
                usage.total_tokens,
            )
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!("failed to persist workflow token usage: {error}"))
            })?;
        if status.is_some_and(codex_protocol::protocol::WorkflowTokenBudgetStatus::exhausted) {
            return Err(CodexErr::WorkflowTokenBudgetExceeded);
        }
        Ok(())
    }

    pub(crate) async fn extend_workflow_token_budget(
        &self,
        turn_context: &TurnContext,
        percent: u8,
    ) -> CodexResult<()> {
        if !WORKFLOW_TOKEN_BUDGET_EXTENSION_PERCENTS.contains(&percent) {
            return Err(CodexErr::InvalidRequest(format!(
                "workflow token budget extension must be one of 25, 50, 75, or 100 percent; got {percent}"
            )));
        }
        let root_thread_id = self
            .services
            .agent_control
            .root_thread_id_for(self.thread_id)
            .await;
        let status = self
            .services
            .agent_control
            .workflow_token_budget()
            .extend(
                self.services.state_db.as_ref(),
                root_thread_id,
                i64::from(percent),
            )
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!("failed to extend workflow token budget: {error}"))
            })?;
        if status.is_none() {
            return Err(CodexErr::InvalidRequest(
                "workflow token budget can only be extended after the active limit is reached"
                    .to_string(),
            ));
        }
        self.send_token_count_event(turn_context).await;
        Ok(())
    }

    pub(crate) async fn close_workflow_token_budget(
        &self,
        turn_context: &TurnContext,
    ) -> CodexResult<()> {
        let root_thread_id = self
            .services
            .agent_control
            .root_thread_id_for(self.thread_id)
            .await;
        self.services
            .agent_control
            .workflow_token_budget()
            .close(self.services.state_db.as_ref(), root_thread_id)
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!("failed to close workflow token budget: {error}"))
            })?;
        self.send_token_count_event(turn_context).await;
        Ok(())
    }
}
