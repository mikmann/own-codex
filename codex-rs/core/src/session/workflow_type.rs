use crate::config::Config;
use crate::session::session::Session;
use codex_config::ConstraintError;
use codex_config::RequirementSource;
use codex_protocol::config_types::WorkflowRequestClass;
use codex_protocol::config_types::WorkflowType;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::RolloutItem;

pub(super) fn initial_workflow_state(
    config: &Config,
    initial_history: &InitialHistory,
) -> (WorkflowType, Option<WorkflowRequestClass>) {
    if !config.extended_workflow.enabled {
        return (WorkflowType::Inactive, None);
    }
    if config.extended_workflow.workflow_type_from_session_flags {
        return (config.extended_workflow.workflow_type, None);
    }
    restored_workflow_state(initial_history)
        .unwrap_or((config.extended_workflow.workflow_type, None))
}

pub(super) fn validate_update(
    config: &Config,
    workflow_type: WorkflowType,
) -> Result<(), ConstraintError> {
    if config.extended_workflow.enabled || workflow_type == WorkflowType::Inactive {
        return Ok(());
    }
    Err(ConstraintError::InvalidValue {
        field_name: "extended_workflow.workflow_type",
        candidate: workflow_type.to_string(),
        allowed: "{inactive} while extended_workflow.enabled=false".to_string(),
        requirement_source: RequirementSource::Unknown,
    })
}

fn restored_workflow_state(
    initial_history: &InitialHistory,
) -> Option<(WorkflowType, Option<WorkflowRequestClass>)> {
    initial_history
        .get_rollout_items()
        .iter()
        .rev()
        .find_map(|item| match item {
            RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event)) => {
                event.thread_settings.workflow_type.map(|workflow_type| {
                    (workflow_type, event.thread_settings.effective_request_class)
                })
            }
            _ => None,
        })
}

impl Session {
    pub(super) async fn set_effective_workflow_request_class(
        &self,
        request_class: WorkflowRequestClass,
    ) {
        let mut state = self.state.lock().await;
        state.session_configuration.effective_request_class = Some(request_class);
    }
}

#[cfg(test)]
#[path = "workflow_type_tests.rs"]
mod tests;
