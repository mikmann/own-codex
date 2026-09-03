use super::*;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ResumedHistory;
use codex_protocol::protocol::ThreadSettingsAppliedEvent;
use codex_protocol::protocol::ThreadSettingsSnapshot;
use std::sync::Arc;

#[test]
fn restored_workflow_type_uses_latest_explicit_snapshot() {
    let mut first = test_snapshot();
    first.workflow_type = Some(WorkflowType::Concept);
    let mut second = test_snapshot();
    second.workflow_type = Some(WorkflowType::WorkOrder);
    second.effective_request_class = Some(WorkflowRequestClass::WorkOrder);
    let rollout_items = vec![
        RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
            ThreadSettingsAppliedEvent {
                thread_settings: first,
            },
        )),
        RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
            ThreadSettingsAppliedEvent {
                thread_settings: second,
            },
        )),
    ];

    for history in [
        InitialHistory::Forked(rollout_items.clone()),
        InitialHistory::Resumed(ResumedHistory {
            conversation_id: ThreadId::default(),
            history: Arc::new(rollout_items),
            rollout_path: None,
        }),
    ] {
        assert_eq!(
            restored_workflow_state(&history),
            Some((
                WorkflowType::WorkOrder,
                Some(WorkflowRequestClass::WorkOrder)
            ))
        );
    }
}

fn test_snapshot() -> ThreadSettingsSnapshot {
    serde_json::from_value(serde_json::json!({
        "model": "test-model",
        "model_provider_id": "test-provider",
        "service_tier": null,
        "approval_policy": "never",
        "approvals_reviewer": "user",
        "permission_profile": {"type": "disabled"},
        "cwd": std::env::current_dir().expect("cwd"),
        "reasoning_effort": null,
        "reasoning_summary": null,
        "personality": null,
        "collaboration_mode": {
            "mode": "default",
            "settings": {"model": "test-model", "reasoning_effort": null}
        }
    }))
    .expect("test thread settings snapshot")
}
