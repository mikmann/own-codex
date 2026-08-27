use super::*;

impl ChatWidget {
    pub(super) fn maybe_prompt_for_workflow_budget_extension(&mut self) {
        let Some(status) = self.workflow_token_budget else {
            return;
        };
        if !status.active
            || status.base_limit_tokens == 0
            || status.workflow_tokens_used < status.effective_limit_tokens
        {
            self.workflow_budget_prompted_limit = None;
            return;
        }
        if self.workflow_budget_prompted_limit == Some(status.effective_limit_tokens) {
            return;
        }
        self.workflow_budget_prompted_limit = Some(status.effective_limit_tokens);

        let items = [100_u8, 75, 50, 25]
            .into_iter()
            .map(|percent| {
                let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                    tx.send(AppEvent::CodexOp(AppCommand::ExtendWorkflowTokenBudget {
                        percent,
                    }));
                })];
                SelectionItem {
                    name: format!("+{percent}% des Basislimits"),
                    description: Some(format!(
                        "Erhöht die Obergrenze um {} Tokens; der Verbrauch wird nicht zurückgesetzt.",
                        status_surfaces::format_workflow_tokens(
                            status.base_limit_tokens.saturating_mul(i64::from(percent)) / 100
                        )
                    )),
                    actions,
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .chain(std::iter::once(SelectionItem {
                name: "Stop".to_string(),
                description: Some(
                    "Keine weiteren Tokens freigeben; der Workflow bleibt angehalten.".to_string(),
                ),
                dismiss_on_select: true,
                ..Default::default()
            }))
            .collect();

        self.show_selection_view(SelectionViewParams {
            title: Some("Workflow-Tokenlimit erreicht".to_string()),
            subtitle: Some(format!(
                "Verbraucht: {} / {} Tokens. Weitere Tokens freigeben?",
                status_surfaces::format_workflow_tokens(status.workflow_tokens_used),
                status_surfaces::format_workflow_tokens(status.effective_limit_tokens)
            )),
            footer_hint: Some(standard_popup_hint_line()),
            initial_selected_idx: Some(0),
            items,
            ..Default::default()
        });
    }
}
