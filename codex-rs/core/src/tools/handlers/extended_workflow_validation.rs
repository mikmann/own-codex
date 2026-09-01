use crate::extended_workflow::ContractValidation;
use crate::extended_workflow::FindingSeverity;
use crate::extended_workflow::ReviewVerdict;
use codex_agent_graph_store::WorkflowValidationDetails;
use codex_protocol::protocol::WorkflowAgentValidationDisplay;
use codex_protocol::protocol::WorkflowFindingSeverity;
use codex_protocol::protocol::WorkflowReviewerVerdict;
use codex_protocol::protocol::WorkflowValidationErrorCode;
use codex_protocol::protocol::WorkflowValidationFormalStatus;
use codex_protocol::protocol::WorkflowValidationGateStatus;

const MAX_VALIDATION_MESSAGE_BYTES: usize = 512;

pub(crate) struct WorkflowValidationPresentation {
    pub(crate) stored: WorkflowValidationDetails,
    pub(crate) display: WorkflowAgentValidationDisplay,
}

pub(crate) struct WorkflowValidationPresentationArgs<'a> {
    pub(crate) validation: &'a ContractValidation,
    pub(crate) reviewer_verdict: Option<ReviewVerdict>,
    pub(crate) submission: u8,
    pub(crate) retry_allowed: bool,
    pub(crate) change_digest_sha256: Option<String>,
    pub(crate) passing_reviewers: Option<u32>,
    pub(crate) required_reviewers: Option<u32>,
}

pub(crate) fn validation_presentation(
    args: WorkflowValidationPresentationArgs<'_>,
) -> WorkflowValidationPresentation {
    let formal_status = if args.validation.valid {
        WorkflowValidationFormalStatus::Valid
    } else {
        WorkflowValidationFormalStatus::Invalid
    };
    let gate_status = if !args.validation.valid {
        WorkflowValidationGateStatus::Excluded
    } else if args
        .reviewer_verdict
        .is_some_and(|verdict| verdict != ReviewVerdict::Pass)
        || args.validation.finding_count > 0
    {
        WorkflowValidationGateStatus::NeedsResolution
    } else {
        WorkflowValidationGateStatus::Eligible
    };
    let error_code =
        (!args.validation.valid).then(|| validation_error_code(&args.validation.errors));
    let message = (!args.validation.errors.is_empty())
        .then(|| bounded_message(&args.validation.errors.join("; ")));
    let reviewer_verdict = args.reviewer_verdict.map(protocol_verdict);
    let max_severity = args.validation.max_severity.map(protocol_severity);
    let display = WorkflowAgentValidationDisplay {
        formal_status,
        reviewer_verdict,
        gate_status,
        error_code,
        message: message.clone(),
        submission: args.submission,
        max_submissions: 2,
        retry_allowed: args.retry_allowed,
        finding_count: u32::try_from(args.validation.finding_count).unwrap_or(u32::MAX),
        max_severity,
        change_digest_sha256: args.change_digest_sha256.clone(),
        passing_reviewers: args.passing_reviewers,
        required_reviewers: args.required_reviewers,
    };
    let stored = WorkflowValidationDetails {
        reviewer_verdict: reviewer_verdict.map(|verdict| {
            match verdict {
                WorkflowReviewerVerdict::Pass => "pass",
                WorkflowReviewerVerdict::NeedsChanges => "needs_changes",
                WorkflowReviewerVerdict::Blocked => "blocked",
            }
            .to_string()
        }),
        gate_status: match gate_status {
            WorkflowValidationGateStatus::Eligible => "eligible",
            WorkflowValidationGateStatus::NeedsResolution => "needs_resolution",
            WorkflowValidationGateStatus::Excluded => "excluded",
        }
        .to_string(),
        validation_error_code: error_code.map(|code| error_code_name(code).to_string()),
        validation_message: message,
        finding_count: display.finding_count,
        max_severity: max_severity.map(|severity| format!("{severity:?}")),
        change_digest_sha256: args.change_digest_sha256,
    };
    WorkflowValidationPresentation { stored, display }
}

fn validation_error_code(errors: &[String]) -> WorkflowValidationErrorCode {
    let errors = errors.join(" ").to_lowercase();
    if errors.contains("internal workflow validation failure") {
        WorkflowValidationErrorCode::TechnicalFailure
    } else if errors.contains("digest") {
        WorkflowValidationErrorCode::ChangeDigestMismatch
    } else if errors.contains("different root request")
        || errors.contains("assigned workflow task")
        || errors.contains("binding")
    {
        WorkflowValidationErrorCode::TaskBindingMismatch
    } else if errors.contains("terminal json")
        || errors.contains("terminal response")
        || errors.contains("exactly match")
    {
        WorkflowValidationErrorCode::TerminalOutputMismatch
    } else if errors.contains("hard limit") || errors.contains("bytes") {
        WorkflowValidationErrorCode::SizeLimitExceeded
    } else if errors.contains("evidence") {
        WorkflowValidationErrorCode::MissingEvidence
    } else if errors.contains("does not satisfy") || errors.contains("unknown field") {
        WorkflowValidationErrorCode::InvalidReportFormat
    } else if errors.contains("severity")
        || errors.contains("verdict")
        || errors.contains("findings array")
    {
        WorkflowValidationErrorCode::SchemaConsistency
    } else {
        WorkflowValidationErrorCode::ContractViolation
    }
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

fn protocol_verdict(verdict: ReviewVerdict) -> WorkflowReviewerVerdict {
    match verdict {
        ReviewVerdict::Pass => WorkflowReviewerVerdict::Pass,
        ReviewVerdict::NeedsChanges => WorkflowReviewerVerdict::NeedsChanges,
        ReviewVerdict::Blocked => WorkflowReviewerVerdict::Blocked,
    }
}

fn protocol_severity(severity: FindingSeverity) -> WorkflowFindingSeverity {
    match severity {
        FindingSeverity::Low => WorkflowFindingSeverity::Low,
        FindingSeverity::Medium => WorkflowFindingSeverity::Medium,
        FindingSeverity::High => WorkflowFindingSeverity::High,
        FindingSeverity::Critical => WorkflowFindingSeverity::Critical,
    }
}

fn bounded_message(message: &str) -> String {
    if message.len() <= MAX_VALIDATION_MESSAGE_BYTES {
        return message.to_string();
    }
    let mut end = MAX_VALIDATION_MESSAGE_BYTES - '…'.len_utf8();
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &message[..end])
}

#[cfg(test)]
#[path = "extended_workflow_validation_tests.rs"]
mod tests;
