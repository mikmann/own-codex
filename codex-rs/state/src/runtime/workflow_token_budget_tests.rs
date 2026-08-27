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
    let workflow_id = started.workflow_id.clone();
    assert_eq!(
        started,
        WorkflowTokenBudgetRecord {
            workflow_id: workflow_id.clone(),
            base_limit_tokens: 1_000,
            effective_limit_tokens: 1_000,
            workflow_tokens_used: 0,
            session_tokens_used: 0,
            active: true,
        }
    );

    let (first, second) = tokio::join!(
        runtime.record_workflow_token_usage(root_thread_id, &workflow_id, 400),
        runtime.record_workflow_token_usage(root_thread_id, &workflow_id, 700),
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
            workflow_id: workflow_id.clone(),
            base_limit_tokens: 1_000,
            effective_limit_tokens: 1_500,
            workflow_tokens_used: 1_100,
            session_tokens_used: 1_100,
            active: true,
        }
    );

    let closed = runtime
        .close_workflow_token_budget(root_thread_id)
        .await?
        .expect("active workflow should close");
    assert!(!closed.active);
    assert_eq!(
        runtime.close_workflow_token_budget(root_thread_id).await?,
        None,
        "an inactive workflow must not report a second close"
    );
    let next = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 2_000)
        .await?;
    assert_ne!(next.workflow_id, workflow_id);
    let next_workflow_id = next.workflow_id.clone();
    assert_eq!(
        next,
        WorkflowTokenBudgetRecord {
            workflow_id: next_workflow_id,
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
    let started = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 16_000)
        .await?;
    runtime
        .record_workflow_token_usage(root_thread_id, &started.workflow_id, 5_000)
        .await?;

    let resumed = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 32_000)
        .await?;

    assert_eq!(resumed.base_limit_tokens, 16_000);
    assert_eq!(resumed.effective_limit_tokens, 16_000);
    assert_eq!(resumed.workflow_tokens_used, 5_000);
    Ok(())
}

#[tokio::test]
async fn start_outcome_distinguishes_new_and_resumed_generations() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let root_thread_id = ThreadId::new();

    let first = runtime
        .start_or_resume_workflow_token_budget_with_outcome(root_thread_id, 1_000)
        .await?;
    let WorkflowTokenBudgetStartOutcome::StartedNew(first) = first else {
        panic!("the first generation must be reported as new");
    };
    let resumed = runtime
        .start_or_resume_workflow_token_budget_with_outcome(root_thread_id, 2_000)
        .await?;
    let WorkflowTokenBudgetStartOutcome::Resumed(resumed) = resumed else {
        panic!("an active generation must be reported as resumed");
    };

    assert_eq!(resumed.workflow_id, first.workflow_id);
    assert_eq!(resumed.base_limit_tokens, 1_000);
    Ok(())
}

#[tokio::test]
async fn stale_workflow_usage_only_updates_the_session_total() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let root_thread_id = ThreadId::new();
    let first = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 1_000)
        .await?;
    runtime.close_workflow_token_budget(root_thread_id).await?;
    let second = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 2_000)
        .await?;

    assert_ne!(first.workflow_id, second.workflow_id);
    assert_eq!(
        runtime
            .close_workflow_token_budget_generation(root_thread_id, &first.workflow_id)
            .await?,
        None,
        "a stale generation must not close the active workflow"
    );
    assert_eq!(
        runtime
            .record_workflow_token_usage(root_thread_id, &first.workflow_id, 700)
            .await?,
        None
    );
    assert_eq!(
        runtime.workflow_token_budget(root_thread_id).await?,
        Some(WorkflowTokenBudgetRecord {
            workflow_id: second.workflow_id,
            base_limit_tokens: 2_000,
            effective_limit_tokens: 2_000,
            workflow_tokens_used: 0,
            session_tokens_used: 700,
            active: true,
        })
    );
    Ok(())
}

#[tokio::test]
async fn workflow_budget_can_only_extend_once_exhausted() -> anyhow::Result<()> {
    let runtime = runtime().await?;
    let root_thread_id = ThreadId::new();
    let started = runtime
        .start_or_resume_workflow_token_budget(root_thread_id, 1_000)
        .await?;

    assert_eq!(
        runtime
            .extend_workflow_token_budget(root_thread_id, 100)
            .await?,
        None
    );
    runtime
        .record_workflow_token_usage(root_thread_id, &started.workflow_id, 1_000)
        .await?;
    assert_eq!(
        runtime
            .extend_workflow_token_budget(root_thread_id, 10)
            .await?,
        None
    );
    let extended = runtime
        .extend_workflow_token_budget(root_thread_id, 100)
        .await?
        .expect("exhausted budget should extend");
    assert_eq!(extended.effective_limit_tokens, 2_000);
    assert_eq!(
        runtime
            .extend_workflow_token_budget(root_thread_id, 100)
            .await?,
        None
    );
    Ok(())
}
