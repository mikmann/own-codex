use strum::AsRefStr;
use strum::Display;
use strum::EnumString;

/// Status attached to a directional thread-spawn edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum DirectionalThreadSpawnEdgeStatus {
    Open,
    Closing,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValidationSubmission {
    pub root_thread_id: codex_protocol::ThreadId,
    pub turn_id: String,
    pub validation_key: String,
    pub result_kind: String,
    pub agent_thread_id: Option<codex_protocol::ThreadId>,
    pub reviewer_role: Option<String>,
    pub output_revision: u32,
    pub output_sha256: String,
    pub requires_higher_revision: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowValidationDecision {
    Attempt(u8),
    AlreadyAccepted(u8),
    Exhausted(u8),
    UnchangedCorrection(u8),
    ResultLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValidationCompletion {
    pub root_thread_id: codex_protocol::ThreadId,
    pub turn_id: String,
    pub validation_key: String,
    pub submission: u8,
    pub output_revision: u32,
    pub output_sha256: String,
    pub reviewer_pass: bool,
    pub has_high_or_critical: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowReviewerValidation {
    pub agent_thread_id: codex_protocol::ThreadId,
    pub role: String,
    pub output_revision: u32,
    pub output_sha256: String,
    pub reviewer_pass: bool,
    pub has_high_or_critical: bool,
}
