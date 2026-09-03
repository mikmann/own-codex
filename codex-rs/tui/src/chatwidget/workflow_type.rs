use super::*;
use codex_protocol::config_types::WorkflowType;

impl ChatWidget {
    pub(crate) fn open_workflow_type_popup(&mut self) {
        if !self.workflow_type_change_available() {
            return;
        }

        let current = self.selected_workflow_type();
        let items = [
            WorkflowType::Auto,
            WorkflowType::Inactive,
            WorkflowType::Concept,
            WorkflowType::WorkOrder,
        ]
        .into_iter()
        .map(|workflow_type| {
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::UpdateWorkflowType(workflow_type));
            })];
            SelectionItem {
                name: workflow_type_label(workflow_type).to_string(),
                description: Some(workflow_type_description(workflow_type).to_string()),
                is_current: current == workflow_type,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

        let mut header = ColumnRenderable::new();
        header.push(Line::from("Workflow-Typ auswählen".bold()));
        header.push(Line::from(
            "Die Auswahl gilt ab der nächsten Root-Anfrage.".dim(),
        ));
        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn select_workflow_type_from_arg(&mut self, value: &str) {
        let workflow_type = match WorkflowType::parse(value.trim()) {
            Ok(workflow_type) => workflow_type,
            Err(message) => {
                self.add_error_message(message);
                return;
            }
        };
        if !self.workflow_type_change_available() {
            return;
        }
        self.app_event_tx
            .send(AppEvent::UpdateWorkflowType(workflow_type));
    }

    pub(crate) fn set_workflow_type(&mut self, workflow_type: WorkflowType) {
        self.config.extended_workflow.workflow_type = workflow_type;
        self.config.extended_workflow.effective_request_class = None;
        self.refresh_status_surfaces();
        self.add_info_message(
            format!("Workflow-Typ auf {workflow_type} gesetzt. Gilt ab der nächsten Root-Anfrage."),
            /*hint*/ None,
        );
    }

    pub(super) fn selected_workflow_type(&self) -> WorkflowType {
        if self.config.extended_workflow.enabled {
            self.config.extended_workflow.workflow_type
        } else {
            WorkflowType::Inactive
        }
    }

    pub(super) fn workflow_status_line_text(&self) -> String {
        match (self.selected_workflow_type(), self.workflow_token_budget) {
            (WorkflowType::Inactive, _) => "Workflow: inaktiv".to_string(),
            (workflow_type, Some(status))
                if status.active && status.effective_limit_tokens == 0 =>
            {
                format!(
                    "Workflow: {workflow_type} · {} / ∞ Tokens",
                    super::status_surfaces::format_workflow_tokens(status.workflow_tokens_used)
                )
            }
            (workflow_type, Some(status)) if status.active => {
                let percent = status.workflow_tokens_used as f64
                    / status.effective_limit_tokens.max(1) as f64
                    * 100.0;
                let percent = format!("{percent:.1}").replace('.', ",");
                format!(
                    "Workflow: {workflow_type} · {} / {} Tokens ({percent} %)",
                    super::status_surfaces::format_workflow_tokens(status.workflow_tokens_used),
                    super::status_surfaces::format_workflow_tokens(status.effective_limit_tokens)
                )
            }
            (workflow_type, Some(_) | None) => format!("Workflow: {workflow_type}"),
        }
    }

    fn workflow_type_change_available(&mut self) -> bool {
        if !self.is_session_configured() {
            self.add_error_message(
                "Workflow type selection is disabled until startup completes.".to_string(),
            );
            return false;
        }
        if !self.config.extended_workflow.enabled {
            self.add_error_message(
                "Extended Workflow is disabled by extended_workflow.enabled=false; only inactive is effective."
                    .to_string(),
            );
            return false;
        }
        if self.bottom_pane.is_task_running() {
            self.add_error_message(
                "Der Workflow-Typ kann während eines laufenden Turns nicht geändert werden."
                    .to_string(),
            );
            return false;
        }
        if self
            .workflow_token_budget
            .is_some_and(|status| status.active)
        {
            self.add_error_message(
                "Der Workflow-Typ kann bei einer offenen Workflow-Generation nicht geändert werden. Schließe sie ab oder verwende /workflow-reset-budget."
                    .to_string(),
            );
            return false;
        }
        true
    }
}

fn workflow_type_label(workflow_type: WorkflowType) -> &'static str {
    match workflow_type {
        WorkflowType::Auto => "Auto",
        WorkflowType::Inactive => "Inaktiv",
        WorkflowType::Concept => "Konzept",
        WorkflowType::WorkOrder => "Arbeitsauftrag",
    }
}

fn workflow_type_description(workflow_type: WorkflowType) -> &'static str {
    match workflow_type {
        WorkflowType::Auto => "Anfrage automatisch klassifizieren",
        WorkflowType::Inactive => "Extended Workflow für neue Anfragen umgehen",
        WorkflowType::Concept => "Konzept-Workflow erzwingen",
        WorkflowType::WorkOrder => "Arbeitsauftrag-Workflow erzwingen",
    }
}
