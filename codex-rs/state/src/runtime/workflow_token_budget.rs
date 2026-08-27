use crate::StateRuntime;
use codex_protocol::ThreadId;
use sqlx::Row;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowTokenBudgetRecord {
    pub base_limit_tokens: i64,
    pub effective_limit_tokens: i64,
    pub workflow_tokens_used: i64,
    pub session_tokens_used: i64,
    pub active: bool,
}

impl StateRuntime {
    pub async fn start_or_resume_workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
        base_limit_tokens: i64,
    ) -> anyhow::Result<WorkflowTokenBudgetRecord> {
        sqlx::query(
            r#"
INSERT INTO extended_workflow_token_budgets (
    root_thread_id,
    base_limit_tokens,
    effective_limit_tokens,
    active
) VALUES (?, ?, ?, 1)
ON CONFLICT(root_thread_id) DO UPDATE SET
    base_limit_tokens = CASE
        WHEN extended_workflow_token_budgets.active = 0 THEN excluded.base_limit_tokens
        ELSE extended_workflow_token_budgets.base_limit_tokens
    END,
    effective_limit_tokens = CASE
        WHEN extended_workflow_token_budgets.active = 0 THEN excluded.effective_limit_tokens
        ELSE extended_workflow_token_budgets.effective_limit_tokens
    END,
    workflow_tokens_used = CASE
        WHEN extended_workflow_token_budgets.active = 0 THEN 0
        ELSE extended_workflow_token_budgets.workflow_tokens_used
    END,
    active = 1
            "#,
        )
        .bind(root_thread_id.to_string())
        .bind(base_limit_tokens)
        .bind(base_limit_tokens)
        .execute(self.pool.as_ref())
        .await?;
        self.workflow_token_budget(root_thread_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("workflow token budget disappeared after initialization")
            })
    }

    pub async fn record_workflow_token_usage(
        &self,
        root_thread_id: ThreadId,
        tokens: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        sqlx::query(
            r#"
UPDATE extended_workflow_token_budgets
SET workflow_tokens_used = workflow_tokens_used + ?,
    session_tokens_used = session_tokens_used + ?
WHERE root_thread_id = ? AND active = 1
            "#,
        )
        .bind(tokens.max(0))
        .bind(tokens.max(0))
        .bind(root_thread_id.to_string())
        .execute(self.pool.as_ref())
        .await?;
        self.workflow_token_budget(root_thread_id).await
    }

    pub async fn extend_workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
        percent: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        sqlx::query(
            r#"
UPDATE extended_workflow_token_budgets
SET effective_limit_tokens = effective_limit_tokens
    + MAX(1, (base_limit_tokens * ?) / 100)
WHERE root_thread_id = ? AND active = 1 AND base_limit_tokens > 0
            "#,
        )
        .bind(percent)
        .bind(root_thread_id.to_string())
        .execute(self.pool.as_ref())
        .await?;
        self.workflow_token_budget(root_thread_id).await
    }

    pub async fn close_workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        sqlx::query(
            r#"
UPDATE extended_workflow_token_budgets
SET workflow_tokens_used = 0, active = 0
WHERE root_thread_id = ?
            "#,
        )
        .bind(root_thread_id.to_string())
        .execute(self.pool.as_ref())
        .await?;
        self.workflow_token_budget(root_thread_id).await
    }

    pub async fn workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        let row = sqlx::query(
            r#"
SELECT base_limit_tokens,
       effective_limit_tokens,
       workflow_tokens_used,
       session_tokens_used,
       active
FROM extended_workflow_token_budgets
WHERE root_thread_id = ?
            "#,
        )
        .bind(root_thread_id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.map(|row| {
            Ok(WorkflowTokenBudgetRecord {
                base_limit_tokens: row.try_get("base_limit_tokens")?,
                effective_limit_tokens: row.try_get("effective_limit_tokens")?,
                workflow_tokens_used: row.try_get("workflow_tokens_used")?,
                session_tokens_used: row.try_get("session_tokens_used")?,
                active: row.try_get::<i64, _>("active")? != 0,
            })
        })
        .transpose()
    }
}

#[cfg(test)]
#[path = "workflow_token_budget_tests.rs"]
mod tests;
