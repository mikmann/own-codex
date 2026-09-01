use crate::JsonSchema;
use crate::TS;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::WorkflowAgentDisplay as CoreWorkflowAgentDisplay;
use codex_protocol::protocol::WorkflowAgentDisplayStatus as CoreWorkflowAgentDisplayStatus;
use codex_protocol::protocol::WorkflowAgentValidationDisplay as CoreWorkflowAgentValidationDisplay;
use codex_protocol::protocol::WorkflowFindingSeverity as CoreWorkflowFindingSeverity;
use codex_protocol::protocol::WorkflowReviewerVerdict as CoreWorkflowReviewerVerdict;
use codex_protocol::protocol::WorkflowValidationErrorCode as CoreWorkflowValidationErrorCode;
use codex_protocol::protocol::WorkflowValidationFormalStatus as CoreWorkflowValidationFormalStatus;
use codex_protocol::protocol::WorkflowValidationGateStatus as CoreWorkflowValidationGateStatus;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowAgentDisplayStatus {
    Running,
    Validated,
    Rejected,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowValidationFormalStatus {
    Valid,
    Invalid,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowReviewerVerdict {
    Pass,
    NeedsChanges,
    Blocked,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowValidationGateStatus {
    Eligible,
    NeedsResolution,
    Excluded,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowValidationErrorCode {
    TaskBindingMismatch,
    SchemaConsistency,
    InvalidReportFormat,
    MissingEvidence,
    SizeLimitExceeded,
    TerminalOutputMismatch,
    ContractViolation,
    CorrectionExhausted,
    ChangeDigestMismatch,
    TechnicalFailure,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum WorkflowFindingSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub struct WorkflowAgentValidationDisplay {
    pub formal_status: WorkflowValidationFormalStatus,
    pub reviewer_verdict: Option<WorkflowReviewerVerdict>,
    pub gate_status: WorkflowValidationGateStatus,
    pub error_code: Option<WorkflowValidationErrorCode>,
    pub message: Option<String>,
    pub submission: u8,
    pub max_submissions: u8,
    pub retry_allowed: bool,
    pub finding_count: u32,
    pub max_severity: Option<WorkflowFindingSeverity>,
    pub change_digest_sha256: Option<String>,
    pub passing_reviewers: Option<u32>,
    pub required_reviewers: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub struct WorkflowAgentDisplay {
    pub name: String,
    pub agent_type: String,
    pub role: String,
    pub model: String,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub status: WorkflowAgentDisplayStatus,
    pub validation: Option<WorkflowAgentValidationDisplay>,
}

impl From<CoreWorkflowAgentDisplayStatus> for WorkflowAgentDisplayStatus {
    fn from(value: CoreWorkflowAgentDisplayStatus) -> Self {
        match value {
            CoreWorkflowAgentDisplayStatus::Running => Self::Running,
            CoreWorkflowAgentDisplayStatus::Validated => Self::Validated,
            CoreWorkflowAgentDisplayStatus::Rejected => Self::Rejected,
        }
    }
}

impl From<CoreWorkflowAgentValidationDisplay> for WorkflowAgentValidationDisplay {
    fn from(value: CoreWorkflowAgentValidationDisplay) -> Self {
        Self {
            formal_status: match value.formal_status {
                CoreWorkflowValidationFormalStatus::Valid => WorkflowValidationFormalStatus::Valid,
                CoreWorkflowValidationFormalStatus::Invalid => {
                    WorkflowValidationFormalStatus::Invalid
                }
            },
            reviewer_verdict: value.reviewer_verdict.map(|verdict| match verdict {
                CoreWorkflowReviewerVerdict::Pass => WorkflowReviewerVerdict::Pass,
                CoreWorkflowReviewerVerdict::NeedsChanges => WorkflowReviewerVerdict::NeedsChanges,
                CoreWorkflowReviewerVerdict::Blocked => WorkflowReviewerVerdict::Blocked,
            }),
            gate_status: match value.gate_status {
                CoreWorkflowValidationGateStatus::Eligible => {
                    WorkflowValidationGateStatus::Eligible
                }
                CoreWorkflowValidationGateStatus::NeedsResolution => {
                    WorkflowValidationGateStatus::NeedsResolution
                }
                CoreWorkflowValidationGateStatus::Excluded => {
                    WorkflowValidationGateStatus::Excluded
                }
            },
            error_code: value.error_code.map(|code| match code {
                CoreWorkflowValidationErrorCode::TaskBindingMismatch => {
                    WorkflowValidationErrorCode::TaskBindingMismatch
                }
                CoreWorkflowValidationErrorCode::SchemaConsistency => {
                    WorkflowValidationErrorCode::SchemaConsistency
                }
                CoreWorkflowValidationErrorCode::InvalidReportFormat => {
                    WorkflowValidationErrorCode::InvalidReportFormat
                }
                CoreWorkflowValidationErrorCode::MissingEvidence => {
                    WorkflowValidationErrorCode::MissingEvidence
                }
                CoreWorkflowValidationErrorCode::SizeLimitExceeded => {
                    WorkflowValidationErrorCode::SizeLimitExceeded
                }
                CoreWorkflowValidationErrorCode::TerminalOutputMismatch => {
                    WorkflowValidationErrorCode::TerminalOutputMismatch
                }
                CoreWorkflowValidationErrorCode::ContractViolation => {
                    WorkflowValidationErrorCode::ContractViolation
                }
                CoreWorkflowValidationErrorCode::CorrectionExhausted => {
                    WorkflowValidationErrorCode::CorrectionExhausted
                }
                CoreWorkflowValidationErrorCode::ChangeDigestMismatch => {
                    WorkflowValidationErrorCode::ChangeDigestMismatch
                }
                CoreWorkflowValidationErrorCode::TechnicalFailure => {
                    WorkflowValidationErrorCode::TechnicalFailure
                }
            }),
            message: value.message,
            submission: value.submission,
            max_submissions: value.max_submissions,
            retry_allowed: value.retry_allowed,
            finding_count: value.finding_count,
            max_severity: value.max_severity.map(|severity| match severity {
                CoreWorkflowFindingSeverity::Low => WorkflowFindingSeverity::Low,
                CoreWorkflowFindingSeverity::Medium => WorkflowFindingSeverity::Medium,
                CoreWorkflowFindingSeverity::High => WorkflowFindingSeverity::High,
                CoreWorkflowFindingSeverity::Critical => WorkflowFindingSeverity::Critical,
            }),
            change_digest_sha256: value.change_digest_sha256,
            passing_reviewers: value.passing_reviewers,
            required_reviewers: value.required_reviewers,
        }
    }
}

impl From<CoreWorkflowAgentDisplay> for WorkflowAgentDisplay {
    fn from(value: CoreWorkflowAgentDisplay) -> Self {
        Self {
            name: value.name,
            agent_type: value.agent_type,
            role: value.role,
            model: value.model,
            reasoning_effort: value.reasoning_effort,
            status: value.status.into(),
            validation: value.validation.map(Into::into),
        }
    }
}
