use codex_app_server_protocol::WorkflowAgentDisplay;
use codex_app_server_protocol::WorkflowAgentDisplayStatus;
use codex_app_server_protocol::WorkflowAgentValidationDisplay;
use codex_app_server_protocol::WorkflowFindingSeverity;
use codex_app_server_protocol::WorkflowReviewerVerdict;
use codex_app_server_protocol::WorkflowValidationErrorCode;
use codex_app_server_protocol::WorkflowValidationFormalStatus;
use codex_app_server_protocol::WorkflowValidationGateStatus;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

pub(super) fn workflow_agent_status_label(agent: &WorkflowAgentDisplay) -> &'static str {
    if let Some(validation) = agent.validation.as_ref() {
        if validation.formal_status == WorkflowValidationFormalStatus::Invalid {
            return "Report invalid";
        }
        return match (validation.reviewer_verdict, validation.gate_status) {
            (Some(WorkflowReviewerVerdict::Pass), WorkflowValidationGateStatus::Eligible) => "PASS",
            (Some(WorkflowReviewerVerdict::NeedsChanges), _) => "Needs changes",
            (Some(WorkflowReviewerVerdict::Blocked), _) => "Blocked",
            (_, WorkflowValidationGateStatus::Eligible) => "Validated",
            (_, WorkflowValidationGateStatus::NeedsResolution) => "Needs resolution",
            (_, WorkflowValidationGateStatus::Excluded) => "Excluded",
        };
    }
    match agent.status {
        WorkflowAgentDisplayStatus::Running => "Running",
        WorkflowAgentDisplayStatus::Validated => "Validated",
        WorkflowAgentDisplayStatus::Rejected => "Rejected",
    }
}

pub(super) fn workflow_agent_status_span(
    agent: &WorkflowAgentDisplay,
    color_enabled: bool,
) -> Span<'static> {
    let status = workflow_agent_status_label(agent);
    match (agent.validation.as_ref(), agent.status, color_enabled) {
        (Some(validation), _, true)
            if validation.formal_status == WorkflowValidationFormalStatus::Invalid =>
        {
            Span::from(status).red().bold()
        }
        (Some(validation), _, true)
            if validation.gate_status == WorkflowValidationGateStatus::NeedsResolution =>
        {
            Span::from(status).cyan().bold()
        }
        (Some(_), _, true) => Span::from(status).green().bold(),
        (None, WorkflowAgentDisplayStatus::Validated, true) => Span::from(status).green().bold(),
        (None, WorkflowAgentDisplayStatus::Rejected, true) => Span::from(status).red().bold(),
        (None, _, true) => Span::from(status).cyan(),
        (_, _, false) => Span::from(status),
    }
}

pub(super) fn workflow_agent_validation_details(
    validation: &WorkflowAgentValidationDisplay,
) -> Vec<Line<'static>> {
    let formal = match validation.formal_status {
        WorkflowValidationFormalStatus::Valid => "valid",
        WorkflowValidationFormalStatus::Invalid => "invalid",
    };
    let formal_span = match validation.formal_status {
        WorkflowValidationFormalStatus::Valid => Span::from(formal).green().bold(),
        WorkflowValidationFormalStatus::Invalid => Span::from(formal).red().bold(),
    };
    let gate = match validation.gate_status {
        WorkflowValidationGateStatus::Eligible => "ELIGIBLE",
        WorkflowValidationGateStatus::NeedsResolution => "NEEDS RESOLUTION",
        WorkflowValidationGateStatus::Excluded => "EXCLUDED",
    };
    let gate_span = match validation.gate_status {
        WorkflowValidationGateStatus::Eligible => Span::from(gate).green().bold(),
        WorkflowValidationGateStatus::NeedsResolution => Span::from(gate).cyan().bold(),
        WorkflowValidationGateStatus::Excluded => Span::from(gate).red().bold(),
    };
    let mut details = vec![
        vec![Span::from("Formal validation: ").dim(), formal_span].into(),
        vec![Span::from("Gate: ").dim(), gate_span].into(),
    ];
    if let Some(verdict) = validation.reviewer_verdict {
        let verdict = match verdict {
            WorkflowReviewerVerdict::Pass => "PASS",
            WorkflowReviewerVerdict::NeedsChanges => "NEEDS CHANGES",
            WorkflowReviewerVerdict::Blocked => "BLOCKED",
        };
        let verdict_span = match validation.reviewer_verdict {
            Some(WorkflowReviewerVerdict::Pass) => Span::from(verdict).green().bold(),
            Some(WorkflowReviewerVerdict::NeedsChanges) => Span::from(verdict).cyan().bold(),
            Some(WorkflowReviewerVerdict::Blocked) => Span::from(verdict).red().bold(),
            None => Span::from(verdict),
        };
        details.insert(
            1,
            vec![Span::from("Reviewer verdict: ").dim(), verdict_span].into(),
        );
    }
    if validation.finding_count > 0 {
        let severity = match validation.max_severity {
            Some(WorkflowFindingSeverity::Low) => Span::from("Low").dim(),
            Some(WorkflowFindingSeverity::Medium) => Span::from("Medium").cyan().bold(),
            Some(WorkflowFindingSeverity::High) => Span::from("High").red().bold(),
            Some(WorkflowFindingSeverity::Critical) => Span::from("Critical").red().bold(),
            None => Span::from("unknown").dim(),
        };
        details.push(
            vec![
                Span::from("Findings: ").dim(),
                Span::from(validation.finding_count.to_string()),
                Span::from(" | max severity: ").dim(),
                severity,
            ]
            .into(),
        );
    }
    if let (Some(passing), Some(required)) =
        (validation.passing_reviewers, validation.required_reviewers)
    {
        let passed = passing >= required && required > 0;
        let status = if passed { "PASSED" } else { "BLOCKED" };
        let status = if passed {
            Span::from(status).green().bold()
        } else {
            Span::from(status).red().bold()
        };
        details.push(
            vec![
                Span::from("Review round: ").dim(),
                status,
                Span::from(format!(" — {passing}/{required} passing")),
            ]
            .into(),
        );
    }
    if let Some(error_code) = validation.error_code {
        details.push(
            vec![
                Span::from("Reason: ").dim(),
                Span::from(error_code_name(error_code)).red(),
                Span::from(
                    validation
                        .message
                        .as_deref()
                        .map(|message| format!(" — {message}"))
                        .unwrap_or_default(),
                ),
            ]
            .into(),
        );
    }
    if validation.retry_allowed {
        details.push(
            vec![
                Span::from("Correction: ").dim(),
                Span::from(format!(
                    "available ({}/{} submissions used)",
                    validation.submission, validation.max_submissions
                ))
                .cyan(),
            ]
            .into(),
        );
    }
    if let Some(digest) = validation.change_digest_sha256.as_deref() {
        details.push(
            vec![
                Span::from("Diff digest: ").dim(),
                Span::from(digest.chars().take(12).collect::<String>()).cyan(),
            ]
            .into(),
        );
    }
    details
}

fn error_code_name(code: WorkflowValidationErrorCode) -> &'static str {
    match code {
        WorkflowValidationErrorCode::TaskBindingMismatch => "task_binding_mismatch",
        WorkflowValidationErrorCode::SchemaConsistency => "schema_consistency",
        WorkflowValidationErrorCode::InvalidReportFormat => "invalid_report_format",
        WorkflowValidationErrorCode::MissingEvidence => "missing_evidence",
        WorkflowValidationErrorCode::SizeLimitExceeded => "size_limit_exceeded",
        WorkflowValidationErrorCode::TerminalOutputMismatch => "terminal_output_mismatch",
        WorkflowValidationErrorCode::ContractViolation => "contract_violation",
        WorkflowValidationErrorCode::CorrectionExhausted => "correction_exhausted",
        WorkflowValidationErrorCode::ChangeDigestMismatch => "change_digest_mismatch",
        WorkflowValidationErrorCode::TechnicalFailure => "technical_failure",
    }
}
