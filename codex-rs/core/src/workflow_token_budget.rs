use codex_protocol::ThreadId;
use codex_protocol::protocol::WorkflowTokenBudgetStatus;
use codex_rollout::state_db::StateDbHandle;
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct WorkflowTokenBudget {
    fallback: Mutex<Option<WorkflowTokenBudgetStatus>>,
}

impl WorkflowTokenBudget {
    pub(crate) async fn start_or_resume(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
        base_limit_tokens: i64,
    ) -> anyhow::Result<WorkflowTokenBudgetStatus> {
        if let Some(state_db) = state_db {
            return state_db
                .start_or_resume_workflow_token_budget(root_thread_id, base_limit_tokens)
                .await
                .map(workflow_token_budget_status);
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = fallback.get_or_insert(WorkflowTokenBudgetStatus {
            session_tokens_used: 0,
            workflow_tokens_used: 0,
            base_limit_tokens,
            effective_limit_tokens: base_limit_tokens,
            active: true,
        });
        if !state.active {
            state.workflow_tokens_used = 0;
            state.base_limit_tokens = base_limit_tokens;
            state.effective_limit_tokens = base_limit_tokens;
            state.active = true;
        }
        Ok(*state)
    }

    pub(crate) async fn record_usage(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
        tokens: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetStatus>> {
        if let Some(state_db) = state_db {
            return state_db
                .record_workflow_token_usage(root_thread_id, tokens.max(0))
                .await
                .map(|record| record.map(workflow_token_budget_status));
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(state) = fallback.as_mut() else {
            return Ok(None);
        };
        if state.active {
            let tokens = tokens.max(0);
            state.workflow_tokens_used = state.workflow_tokens_used.saturating_add(tokens);
            state.session_tokens_used = state.session_tokens_used.saturating_add(tokens);
        }
        Ok(Some(*state))
    }

    pub(crate) async fn extend(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
        percent: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetStatus>> {
        if let Some(state_db) = state_db {
            return state_db
                .extend_workflow_token_budget(root_thread_id, percent)
                .await
                .map(|record| record.map(workflow_token_budget_status));
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(state) = fallback.as_mut() else {
            return Ok(None);
        };
        if state.active && state.base_limit_tokens > 0 {
            let increment = state
                .base_limit_tokens
                .saturating_mul(percent)
                .saturating_div(100)
                .max(1);
            state.effective_limit_tokens = state.effective_limit_tokens.saturating_add(increment);
        }
        Ok(Some(*state))
    }

    pub(crate) async fn close(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetStatus>> {
        if let Some(state_db) = state_db {
            return state_db
                .close_workflow_token_budget(root_thread_id)
                .await
                .map(|record| record.map(workflow_token_budget_status));
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = fallback.as_mut() {
            state.workflow_tokens_used = 0;
            state.active = false;
        }
        Ok(*fallback)
    }

    pub(crate) async fn status(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetStatus>> {
        if let Some(state_db) = state_db {
            return state_db
                .workflow_token_budget(root_thread_id)
                .await
                .map(|record| record.map(workflow_token_budget_status));
        }
        Ok(*self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

fn workflow_token_budget_status(
    value: codex_state::WorkflowTokenBudgetRecord,
) -> WorkflowTokenBudgetStatus {
    WorkflowTokenBudgetStatus {
        session_tokens_used: value.session_tokens_used,
        workflow_tokens_used: value.workflow_tokens_used,
        base_limit_tokens: value.base_limit_tokens,
        effective_limit_tokens: value.effective_limit_tokens,
        active: value.active,
    }
}
