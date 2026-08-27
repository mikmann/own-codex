use crate::StateRuntime;
use codex_protocol::ThreadId;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTokenBudgetRecord {
    pub workflow_id: String,
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
        let workflow_id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query(
            r#"
INSERT INTO extended_workflow_token_budgets (
    root_thread_id,
    workflow_id,
    base_limit_tokens,
    effective_limit_tokens,
    active
) VALUES (?, ?, ?, ?, 1)
ON CONFLICT(root_thread_id) DO UPDATE SET
    workflow_id = CASE
        WHEN extended_workflow_token_budgets.active = 0 THEN excluded.workflow_id
        ELSE extended_workflow_token_budgets.workflow_id
    END,
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
RETURNING workflow_id,
          base_limit_tokens,
          effective_limit_tokens,
          workflow_tokens_used,
          session_tokens_used,
          active
            "#,
        )
        .bind(root_thread_id.to_string())
        .bind(workflow_id)
        .bind(base_limit_tokens)
        .bind(base_limit_tokens)
        .fetch_one(self.pool.as_ref())
        .await?;
        workflow_token_budget_record(&row)
    }

    pub async fn record_workflow_token_usage(
        &self,
        root_thread_id: ThreadId,
        workflow_id: &str,
        tokens: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        let tokens = tokens.max(0);
        let row = sqlx::query(
            r#"
UPDATE extended_workflow_token_budgets
SET workflow_tokens_used = CASE
        WHEN active = 1 AND workflow_id = ? THEN
            CASE
                WHEN workflow_tokens_used > ? - ? THEN ?
                ELSE workflow_tokens_used + ?
            END
        ELSE workflow_tokens_used
    END,
    session_tokens_used = CASE
        WHEN session_tokens_used > ? - ? THEN ?
        ELSE session_tokens_used + ?
    END
WHERE root_thread_id = ?
RETURNING workflow_id,
          base_limit_tokens,
          effective_limit_tokens,
          workflow_tokens_used,
          session_tokens_used,
          active,
          active = 1 AND workflow_id = ? AS recorded
            "#,
        )
        .bind(workflow_id)
        .bind(i64::MAX)
        .bind(tokens)
        .bind(i64::MAX)
        .bind(tokens)
        .bind(i64::MAX)
        .bind(tokens)
        .bind(i64::MAX)
        .bind(tokens)
        .bind(root_thread_id.to_string())
        .bind(workflow_id)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.filter(|row| row.get::<i64, _>("recorded") != 0)
            .map(|row| workflow_token_budget_record(&row))
            .transpose()
    }

    pub async fn extend_workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
        percent: i64,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        if !matches!(percent, 25 | 50 | 75 | 100) {
            return Ok(None);
        }
        let Some(current) = self.workflow_token_budget(root_thread_id).await? else {
            return Ok(None);
        };
        if !current.active
            || current.base_limit_tokens <= 0
            || current.workflow_tokens_used < current.effective_limit_tokens
        {
            return Ok(None);
        }
        let increment = percentage_increment(current.base_limit_tokens, percent);
        let effective_limit_tokens = current.effective_limit_tokens.saturating_add(increment);
        if effective_limit_tokens == current.effective_limit_tokens {
            return Ok(None);
        }
        let row = sqlx::query(
            r#"
UPDATE extended_workflow_token_budgets
SET effective_limit_tokens = ?
WHERE root_thread_id = ?
  AND workflow_id = ?
  AND active = 1
  AND effective_limit_tokens = ?
  AND workflow_tokens_used >= effective_limit_tokens
RETURNING workflow_id,
          base_limit_tokens,
          effective_limit_tokens,
          workflow_tokens_used,
          session_tokens_used,
          active
            "#,
        )
        .bind(effective_limit_tokens)
        .bind(root_thread_id.to_string())
        .bind(&current.workflow_id)
        .bind(current.effective_limit_tokens)
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.map(|row| workflow_token_budget_record(&row))
            .transpose()
    }

    pub async fn close_workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        let row = sqlx::query(
            r#"
UPDATE extended_workflow_token_budgets
SET workflow_tokens_used = 0, active = 0
WHERE root_thread_id = ? AND active = 1
RETURNING workflow_id,
          base_limit_tokens,
          effective_limit_tokens,
          workflow_tokens_used,
          session_tokens_used,
          active
            "#,
        )
        .bind(root_thread_id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await?;
        row.map(|row| workflow_token_budget_record(&row))
            .transpose()
    }

    pub async fn workflow_token_budget(
        &self,
        root_thread_id: ThreadId,
    ) -> anyhow::Result<Option<WorkflowTokenBudgetRecord>> {
        let row = sqlx::query(
            r#"
SELECT base_limit_tokens,
       workflow_id,
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
        row.map(|row| workflow_token_budget_record(&row))
            .transpose()
    }
}

fn workflow_token_budget_record(
    row: &sqlx::sqlite::SqliteRow,
) -> anyhow::Result<WorkflowTokenBudgetRecord> {
    Ok(WorkflowTokenBudgetRecord {
        workflow_id: row.try_get("workflow_id")?,
        base_limit_tokens: row.try_get("base_limit_tokens")?,
        effective_limit_tokens: row.try_get("effective_limit_tokens")?,
        workflow_tokens_used: row.try_get("workflow_tokens_used")?,
        session_tokens_used: row.try_get("session_tokens_used")?,
        active: row.try_get::<i64, _>("active")? != 0,
    })
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

#[cfg(test)]
#[path = "workflow_token_budget_tests.rs"]
mod tests;
