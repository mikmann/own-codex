use crate::legacy_core::config::Config;
use codex_app_server_protocol::WorkflowTokenBudgetStatus;
use codex_protocol::config_types::WorkflowRequestClass;
use codex_protocol::config_types::WorkflowType;

#[derive(Debug, Clone, Copy)]
pub(super) struct WorkflowStatus {
    workflow_type: WorkflowType,
    effective_request_class: Option<WorkflowRequestClass>,
    budget: Option<WorkflowTokenBudgetStatus>,
}

impl WorkflowStatus {
    pub(super) fn new(config: &Config, budget: Option<WorkflowTokenBudgetStatus>) -> Self {
        let workflow_type = if config.extended_workflow.enabled {
            config.extended_workflow.workflow_type
        } else {
            WorkflowType::Inactive
        };
        Self {
            workflow_type,
            effective_request_class: config.extended_workflow.effective_request_class,
            budget,
        }
    }

    pub(super) fn workflow_type_display(self) -> String {
        self.workflow_type.to_string()
    }

    pub(super) fn effective_request_class_display(self) -> String {
        self.effective_request_class
            .map(|request_class| request_class.to_string())
            .unwrap_or_else(|| "none".to_string())
    }

    pub(super) fn budget_display(self) -> String {
        if self.workflow_type == WorkflowType::Inactive {
            return "inactive".to_string();
        }
        let Some(status) = self.budget.filter(|status| status.active) else {
            return "not active".to_string();
        };
        let used = format_tokens(status.workflow_tokens_used);
        if status.effective_limit_tokens == 0 {
            return format!("{used} / unlimited tokens");
        }
        let limit = format_tokens(status.effective_limit_tokens);
        format!("{used} / {limit} tokens")
    }
}

fn format_tokens(tokens: i64) -> String {
    codex_protocol::num_format::format_with_separators(tokens.max(0)).replace(',', ".")
}
