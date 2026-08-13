use codex_protocol::ThreadId;
use serde::Deserialize;
use serde::Serialize;

/// Persisted close reservation for one authorized spawn subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadSpawnSubtreeClose {
    pub previous_edge_status: ThreadSpawnEdgeStatus,
    pub thread_ids_deepest_first: Vec<ThreadId>,
}

/// Immutable input used to reserve one structured workflow-result validation attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowValidationSubmission {
    pub root_thread_id: ThreadId,
    pub turn_id: String,
    pub validation_key: String,
    pub result_kind: String,
    pub agent_thread_id: Option<ThreadId>,
    pub reviewer_role: Option<String>,
    pub output_revision: u32,
    pub output_sha256: String,
    pub requires_higher_revision: bool,
}

/// Atomic decision returned when reserving a workflow-result validation attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowValidationDecision {
    Attempt(u8),
    AlreadyAccepted(u8),
    Exhausted(u8),
    UnchangedCorrection(u8),
    ResultLimitReached,
}

/// State recorded only after the reserved result has passed its native contract validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowValidationCompletion {
    pub root_thread_id: ThreadId,
    pub turn_id: String,
    pub validation_key: String,
    pub submission: u8,
    pub output_revision: u32,
    pub output_sha256: String,
    pub reviewer_pass: bool,
    pub has_high_or_critical: bool,
}

/// Accepted reviewer evidence used by the root turn's completion gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowReviewerValidation {
    pub agent_thread_id: ThreadId,
    pub role: String,
    pub output_revision: u32,
    pub output_sha256: String,
    pub reviewer_pass: bool,
    pub has_high_or_critical: bool,
}

/// Persisted validation state for any structured workflow result in one root turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowValidationRecord {
    pub validation_key: String,
    pub result_kind: String,
    pub agent_thread_id: Option<ThreadId>,
    pub agent_role: Option<String>,
    pub submissions: u8,
    pub accepted: bool,
    pub output_revision: u32,
    pub output_sha256: String,
    pub reviewer_pass: bool,
    pub has_high_or_critical: bool,
}

/// Lifecycle status attached to a directional thread-spawn edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadSpawnEdgeStatus {
    /// The child thread is still live or resumable as an open spawned agent.
    Open,
    /// The child and its subtree are being closed and must not be spawned under or resumed.
    Closing,
    /// The child thread has been closed from the parent/child graph's perspective.
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn thread_spawn_edge_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&ThreadSpawnEdgeStatus::Open)
                .expect("open status should serialize"),
            "\"open\""
        );
        assert_eq!(
            serde_json::to_string(&ThreadSpawnEdgeStatus::Closing)
                .expect("closing status should serialize"),
            "\"closing\""
        );
        assert_eq!(
            serde_json::to_string(&ThreadSpawnEdgeStatus::Closed)
                .expect("closed status should serialize"),
            "\"closed\""
        );
        assert_eq!(
            serde_json::from_str::<ThreadSpawnEdgeStatus>("\"open\"")
                .expect("open status should deserialize"),
            ThreadSpawnEdgeStatus::Open
        );
        assert_eq!(
            serde_json::from_str::<ThreadSpawnEdgeStatus>("\"closing\"")
                .expect("closing status should deserialize"),
            ThreadSpawnEdgeStatus::Closing
        );
        assert_eq!(
            serde_json::from_str::<ThreadSpawnEdgeStatus>("\"closed\"")
                .expect("closed status should deserialize"),
            ThreadSpawnEdgeStatus::Closed
        );
    }
}
