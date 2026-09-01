use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;

use crate::event_mapping::parse_turn_item;

use super::MAX_WORKFLOW_ROOT_REQUEST_BYTES;

const MAX_CONTINUATION_BYTES: usize = 96;
const MAX_PREVIOUS_USER_BYTES: usize = 900;
const MAX_PREVIOUS_ASSISTANT_BYTES: usize = 1_650;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTaskBindingKind {
    Direct,
    ResolvedContinuation,
    NeedsClarification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTaskBinding {
    pub kind: WorkflowTaskBindingKind,
    pub effective_request: String,
    pub message: Option<String>,
}

pub fn resolve_task_binding(input: &str, history: &[ResponseItem]) -> WorkflowTaskBinding {
    if !is_short_continuation(input) {
        return direct_binding(input);
    }

    let conversation = visible_conversation(history);
    let conversation = conversation.strip_current_user_message(input);
    let Some((previous_user, previous_assistant)) = conversation.previous_exchange() else {
        return WorkflowTaskBinding {
            kind: WorkflowTaskBindingKind::NeedsClarification,
            effective_request: input.to_string(),
            message: Some(
                "the short continuation could not be bound to a prior user request and assistant decision; ask the user to restate the selected action before starting experts or reviewers"
                    .to_string(),
            ),
        };
    };

    let previous_user = truncate_utf8(previous_user, MAX_PREVIOUS_USER_BYTES);
    let previous_assistant = truncate_utf8(previous_assistant, MAX_PREVIOUS_ASSISTANT_BYTES);
    let continuation = truncate_utf8(input.trim(), MAX_CONTINUATION_BYTES);
    let effective_request = format!(
        "Resolved continuation task:\nPrevious user request:\n{previous_user}\n\nPrevious assistant decision context:\n{previous_assistant}\n\nUser continuation:\n{continuation}"
    );
    let effective_request = truncate_utf8(&effective_request, MAX_WORKFLOW_ROOT_REQUEST_BYTES);
    WorkflowTaskBinding {
        kind: WorkflowTaskBindingKind::ResolvedContinuation,
        effective_request,
        message: Some(
            "short continuation resolved against the immediately preceding user/assistant exchange"
                .to_string(),
        ),
    }
}

fn direct_binding(input: &str) -> WorkflowTaskBinding {
    WorkflowTaskBinding {
        kind: WorkflowTaskBindingKind::Direct,
        effective_request: input.to_string(),
        message: None,
    }
}

fn is_short_continuation(input: &str) -> bool {
    let normalized = input
        .trim()
        .trim_matches(|character: char| character.is_ascii_punctuation())
        .to_lowercase();
    if normalized.is_empty() || normalized.len() > MAX_CONTINUATION_BYTES {
        return false;
    }
    if normalized
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        return true;
    }
    matches!(
        normalized.as_str(),
        "ja" | "yes"
            | "ok"
            | "okay"
            | "weiter"
            | "mach das"
            | "mache das"
            | "dann los"
            | "bitte umsetzen"
            | "setz das um"
            | "setze das um"
            | "go ahead"
            | "do it"
    ) || normalized.starts_with("option ")
        || normalized.starts_with("bitte option ")
        || normalized.starts_with("ok, dann ")
        || normalized.starts_with("ja, dann ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversationRole {
    User,
    Assistant,
}

#[derive(Debug, Default)]
struct VisibleConversation(Vec<(ConversationRole, String)>);

impl VisibleConversation {
    fn strip_current_user_message(mut self, input: &str) -> Self {
        if self.0.last().is_some_and(|(role, text)| {
            *role == ConversationRole::User && text.trim() == input.trim()
        }) {
            self.0.pop();
        }
        self
    }

    fn previous_exchange(&self) -> Option<(&str, &str)> {
        let assistant_index = self.0.iter().rposition(|(role, text)| {
            *role == ConversationRole::Assistant && !text.trim().is_empty()
        })?;
        let previous_assistant = self.0[assistant_index].1.as_str();
        let previous_user = self.0[..assistant_index]
            .iter()
            .rfind(|(role, text)| *role == ConversationRole::User && !text.trim().is_empty())?
            .1
            .as_str();
        Some((previous_user, previous_assistant))
    }
}

fn visible_conversation(history: &[ResponseItem]) -> VisibleConversation {
    VisibleConversation(
        history
            .iter()
            .filter_map(parse_turn_item)
            .filter_map(|item| match item {
                TurnItem::UserMessage(message) => Some((ConversationRole::User, message.message())),
                TurnItem::AgentMessage(message) => {
                    let text = message
                        .content
                        .into_iter()
                        .map(|content| match content {
                            AgentMessageContent::Text { text } => text,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some((ConversationRole::Assistant, text))
                }
                _ => None,
            })
            .collect(),
    )
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.saturating_sub('…'.len_utf8());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

#[cfg(test)]
#[path = "task_binding_tests.rs"]
mod tests;
