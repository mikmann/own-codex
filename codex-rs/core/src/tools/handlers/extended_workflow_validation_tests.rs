use crate::extended_workflow::ContractValidation;
use crate::extended_workflow::FindingSeverity;
use crate::extended_workflow::ReviewVerdict;
use codex_protocol::protocol::WorkflowValidationErrorCode;
use codex_protocol::protocol::WorkflowValidationGateStatus;
use pretty_assertions::assert_eq;

use super::WorkflowValidationPresentationArgs;
use super::validation_presentation;

#[test]
fn separates_valid_report_verdict_and_gate_status() {
    let validation = ContractValidation {
        valid: true,
        errors: Vec::new(),
        has_high_or_critical: false,
        finding_count: 1,
        max_severity: Some(FindingSeverity::Low),
    };

    let presentation = validation_presentation(WorkflowValidationPresentationArgs {
        validation: &validation,
        reviewer_verdict: Some(ReviewVerdict::NeedsChanges),
        submission: 1,
        retry_allowed: false,
        change_digest_sha256: Some("a".repeat(64)),
        passing_reviewers: Some(1),
        required_reviewers: Some(2),
    });

    assert_eq!(
        presentation.display.gate_status,
        WorkflowValidationGateStatus::NeedsResolution
    );
    assert_eq!(presentation.stored.gate_status, "needs_resolution");
    assert_eq!(presentation.stored.finding_count, 1);
}

#[test]
fn classifies_terminal_output_mismatch() {
    let validation = ContractValidation {
        valid: false,
        errors: vec!["submitted result does not exactly match terminal JSON".to_string()],
        has_high_or_critical: false,
        finding_count: 0,
        max_severity: None,
    };

    let presentation = validation_presentation(WorkflowValidationPresentationArgs {
        validation: &validation,
        reviewer_verdict: Some(ReviewVerdict::Pass),
        submission: 2,
        retry_allowed: false,
        change_digest_sha256: None,
        passing_reviewers: None,
        required_reviewers: None,
    });

    assert_eq!(
        presentation.display.error_code,
        Some(WorkflowValidationErrorCode::TerminalOutputMismatch)
    );
}
