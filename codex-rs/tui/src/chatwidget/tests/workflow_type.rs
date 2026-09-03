use super::*;
use codex_protocol::config_types::WorkflowType;

#[tokio::test]
async fn workflow_type_popup_lists_all_choices_and_marks_auto() {
    let (mut chat, _rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.config.extended_workflow.enabled = true;

    chat.open_workflow_type_popup();

    assert_chatwidget_snapshot!(
        "workflow_type_picker_auto",
        render_bottom_popup(&chat, /*width*/ 82)
    );
}

#[tokio::test]
async fn workflow_type_inline_argument_emits_typed_update() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.config.extended_workflow.enabled = true;

    chat.select_workflow_type_from_arg("work-order");

    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::UpdateWorkflowType(WorkflowType::WorkOrder))
    ));
}

#[tokio::test]
async fn active_workflow_generation_blocks_type_change() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.config.extended_workflow.enabled = true;
    chat.workflow_token_budget = Some(codex_app_server_protocol::WorkflowTokenBudgetStatus {
        session_tokens_used: 10,
        workflow_tokens_used: 10,
        base_limit_tokens: 100,
        effective_limit_tokens: 100,
        active: true,
    });

    chat.select_workflow_type_from_arg("inactive");

    while let Ok(event) = rx.try_recv() {
        assert!(!matches!(event, AppEvent::UpdateWorkflowType(_)));
    }
    assert_eq!(chat.selected_workflow_type(), WorkflowType::Auto);
}
