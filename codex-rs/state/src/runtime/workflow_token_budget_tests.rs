use super::*;
use crate::runtime::test_support::unique_temp_dir;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

async fn runtime() -> anyhow::Result<std::sync::Arc<StateRuntime>> {
    let home = unique_temp_dir();
    StateRuntime::init(
        crate::SqliteConfig::new_for_testing(home.as_path().abs()),
        "test-provider".to_string(),
    )
    .await
}

#[tokio::test]
async fn workflow_budget_persists_lifecycle_and_session_total() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let root_thread_id = ThreadId::new();

    let started = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 1_000)
        .await?;
    assert_eq!(
        started,
        WorkflowTokenBudgetRecord {
            base_limit_tokens: 1_000,
            effective_limit_tokens: 1_000,
            workflow_tokens_used: 0,
            session_tokens_used: 0,
            active: true,
        }
    );

    let (first, second) = tokio::join!(
        runtime.record_workflow_token_usage(root_thread_id, 400),
        runtime.record_workflow_token_usage(root_thread_id, 700),
    );
    first?;
    second?;
    let extended = runtime
        .extend_workflow_token_budget(root_thread_id, 50)
        .await?
        .expect("budget");
    assert_eq!(
        extended,
        WorkflowTokenBudgetRecord {
            base_limit_tokens: 1_000,
            effective_limit_tokens: 1_500,
            workflow_tokens_used: 1_100,
            session_tokens_used: 1_100,
            active: true,
        }
    );

    runtime.close_workflow_token_budget(root_thread_id).await?;
    let next = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 2_000)
        .await?;
    assert_eq!(
        next,
        WorkflowTokenBudgetRecord {
            base_limit_tokens: 2_000,
            effective_limit_tokens: 2_000,
            workflow_tokens_used: 0,
            session_tokens_used: 1_100,
            active: true,
        }
    );
    Ok(())
}

#[tokio::test]
async fn active_workflow_keeps_its_original_limit_on_resume() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let root_thread_id = ThreadId::new();
    runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 16_000)
        .await?;
    runtime
        .record_workflow_token_usage(root_thread_id, 5_000)
        .await?;

    let resumed = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 32_000)
        .await?;

    assert_eq!(resumed.base_limit_tokens, 16_000);
    assert_eq!(resumed.effective_limit_tokens, 16_000);
    assert_eq!(resumed.workflow_tokens_used, 5_000);
    Ok(())
}
