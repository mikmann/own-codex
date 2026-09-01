use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

use super::WorkflowTaskBindingKind;
use super::resolve_task_binding;
use super::truncate_utf8;

fn message(role: &str, text: &str) -> ResponseItem {
    let content = if role == "user" {
        ContentItem::InputText {
            text: text.to_string(),
        }
    } else {
        ContentItem::OutputText {
            text: text.to_string(),
        }
    };
    ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content: vec![content],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn resolves_numeric_continuation_to_previous_exchange() {
    let history = vec![
        message("user", "Welche Korrektur sollen wir umsetzen?"),
        message(
            "assistant",
            "1. Validator und Anzeige gemeinsam korrigieren.",
        ),
        message("user", "1"),
    ];

    let binding = resolve_task_binding("1", &history);

    assert_eq!(binding.kind, WorkflowTaskBindingKind::ResolvedContinuation);
    assert!(binding.effective_request.contains("Validator und Anzeige"));
    assert!(binding.effective_request.contains("User continuation:\n1"));
}

#[test]
fn short_continuation_without_context_requires_clarification() {
    let binding = resolve_task_binding("ja", &[]);

    assert_eq!(binding.kind, WorkflowTaskBindingKind::NeedsClarification);
    assert!(binding.message.is_some());
}

#[test]
fn substantive_request_remains_direct() {
    let input = "Implementiere die additive App-Server-v2-Validierungsanzeige.";
    let binding = resolve_task_binding(input, &[]);

    assert_eq!(binding.kind, WorkflowTaskBindingKind::Direct);
    assert_eq!(binding.effective_request, input);
}

#[test]
fn truncation_preserves_a_valid_utf8_boundary_before_the_ellipsis() {
    assert_eq!(truncate_utf8("ääää", 5), "ä…");
}
