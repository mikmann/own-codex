use codex_protocol::ThreadId;
use codex_protocol::protocol::WorkflowTokenBudgetStatus;
use codex_rollout::state_db::StateDbHandle;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Debug)]
struct WorkflowTokenBudgetSnapshot {
    status: WorkflowTokenBudgetStatus,
    workflow_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkflowTokenBudgetLease {
    Disabled,
    Active { workflow_id: String },
}

#[derive(Default)]
pub(crate) struct WorkflowTokenBudget {
    fallback: Mutex<Option<WorkflowTokenBudgetSnapshot>>,
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
                .map(workflow_token_budget_snapshot)
                .map(|snapshot| snapshot.status);
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = fallback.get_or_insert_with(|| WorkflowTokenBudgetSnapshot {
            status: WorkflowTokenBudgetStatus {
                session_tokens_used: 0,
                workflow_tokens_used: 0,
                base_limit_tokens,
                effective_limit_tokens: base_limit_tokens,
                active: true,
            },
            workflow_id: Uuid::new_v4().to_string(),
        });
        if !snapshot.status.active {
            snapshot.status.workflow_tokens_used = 0;
            snapshot.status.base_limit_tokens = base_limit_tokens;
            snapshot.status.effective_limit_tokens = base_limit_tokens;
            snapshot.status.active = true;
            snapshot.workflow_id = Uuid::new_v4().to_string();
        }
        Ok(snapshot.status)
    }

    pub(crate) async fn lease(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<(WorkflowTokenBudgetStatus, WorkflowTokenBudgetLease)>> {
        let snapshot = self.snapshot(state_db, root_thread_id).await?;
        Ok(snapshot.map(|snapshot| {
            let lease = WorkflowTokenBudgetLease::Active {
                workflow_id: snapshot.workflow_id,
            };
            (snapshot.status, lease)
        }))
    }

    pub(crate) async fn record_usage(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
        lease: &WorkflowTokenBudgetLease,
        tokens: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetStatus>> {
        let WorkflowTokenBudgetLease::Active { workflow_id } = lease else {
            return Ok(None);
        };
        if let Some(state_db) = state_db {
            return state_db
                .record_workflow_token_usage(root_thread_id, workflow_id, tokens.max(0))
                .await
                .map(|record| record.map(|record| workflow_token_budget_status(&record)));
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(snapshot) = fallback.as_mut() else {
            return Ok(None);
        };
        let tokens = tokens.max(0);
        snapshot.status.session_tokens_used =
            snapshot.status.session_tokens_used.saturating_add(tokens);
        if snapshot.status.active && snapshot.workflow_id == *workflow_id {
            snapshot.status.workflow_tokens_used =
                snapshot.status.workflow_tokens_used.saturating_add(tokens);
            return Ok(Some(snapshot.status));
        }
        Ok(None)
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
                .map(|record| record.map(|record| workflow_token_budget_status(&record)));
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(snapshot) = fallback.as_mut() else {
            return Ok(None);
        };
        let status = &mut snapshot.status;
        if !status.active
            || status.base_limit_tokens <= 0
            || status.workflow_tokens_used < status.effective_limit_tokens
        {
            return Ok(None);
        }
        let increment = percentage_increment(status.base_limit_tokens, percent);
        let effective_limit_tokens = status.effective_limit_tokens.saturating_add(increment);
        if effective_limit_tokens == status.effective_limit_tokens {
            return Ok(None);
        }
        status.effective_limit_tokens = effective_limit_tokens;
        Ok(Some(*status))
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
                .map(|record| record.map(|record| workflow_token_budget_status(&record)));
        }
        let mut fallback = self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(snapshot) = fallback.as_mut() {
            snapshot.status.workflow_tokens_used = 0;
            snapshot.status.active = false;
        }
        Ok(fallback.as_ref().map(|snapshot| snapshot.status))
    }

    pub(crate) async fn status(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetStatus>> {
        self.snapshot(state_db, root_thread_id)
            .await
            .map(|snapshot| snapshot.map(|snapshot| snapshot.status))
    }

    async fn snapshot(
        &self,
        state_db: Option<&StateDbHandle>,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetSnapshot>> {
        if let Some(state_db) = state_db {
            return state_db
                .workflow_token_budget(root_thread_id)
                .await
                .map(|record| record.map(workflow_token_budget_snapshot));
        }
        Ok(self
            .fallback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }
}

fn workflow_token_budget_snapshot(
    value: codex_state::WorkflowTokenBudgetRecord,
) -> WorkflowTokenBudgetSnapshot {
    WorkflowTokenBudgetSnapshot {
        status: workflow_token_budget_status(&value),
        workflow_id: value.workflow_id,
    }
}

fn workflow_token_budget_status(
    value: &codex_state::WorkflowTokenBudgetRecord,
) -> WorkflowTokenBudgetStatus {
    WorkflowTokenBudgetStatus {
        session_tokens_used: value.session_tokens_used,
        workflow_tokens_used: value.workflow_tokens_used,
        base_limit_tokens: value.base_limit_tokens,
        effective_limit_tokens: value.effective_limit_tokens,
        active: value.active,
    }
}

fn percentage_increment(base_limit_tokens: i64, percent: i64) -> i64 {
    let whole = base_limit_tokens
        .saturating_div(100)
        .saturating_mul(percent);
    let remainder = base_limit_tokens
        .rem_euclid(100)
        .saturating_mul(percent)
        .saturating_div(100);
    whole.saturating_add(remainder).max(1)
}
