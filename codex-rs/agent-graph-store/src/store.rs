use std::future::Future;
use std::pin::Pin;

use codex_protocol::ThreadId;

use crate::AgentGraphStoreResult;
use crate::ThreadSpawnEdgeStatus;
use crate::ThreadSpawnSubtreeClose;
use crate::WorkflowReviewerValidation;
use crate::WorkflowValidationCompletion;
use crate::WorkflowValidationDecision;
use crate::WorkflowValidationRecord;
use crate::WorkflowValidationSubmission;

/// Future returned by [`AgentGraphStore`] operations.
pub type AgentGraphStoreFuture<'a, T> =
    Pin<Box<dyn Future<Output = AgentGraphStoreResult<T>> + Send + 'a>>;

/// Storage-neutral boundary for persisted thread-spawn parent/child topology.
///
/// Implementations are expected to return stable ordering for list methods so callers can merge
/// persisted graph state with live in-memory state without introducing nondeterministic output.
pub trait AgentGraphStore: Send + Sync {
    /// Insert or replace the directional parent/child edge for a spawned thread.
    ///
    /// `child_thread_id` has at most one persisted parent. Re-inserting the same child should
    /// update both the parent and status to match the supplied values.
    fn upsert_thread_spawn_edge(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        status: ThreadSpawnEdgeStatus,
    ) -> AgentGraphStoreFuture<'_, ()>;

    /// Update the persisted lifecycle status of a spawned thread's incoming edge.
    ///
    /// Implementations should treat missing children as a successful no-op.
    fn set_thread_spawn_edge_status(
        &self,
        child_thread_id: ThreadId,
        status: ThreadSpawnEdgeStatus,
    ) -> AgentGraphStoreFuture<'_, ()>;

    /// Ensure that a spawn edge is open without reopening a closing or closed child.
    ///
    /// Returns `false` when either the child already has a non-open edge or the parent itself is
    /// closing or closed. Implementations must perform the parent check and insert atomically.
    fn ensure_thread_spawn_edge_open(
        &self,
        root_thread_id: ThreadId,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, bool>;

    /// Return the persisted lifecycle status of a spawned thread's incoming edge.
    fn thread_spawn_edge_status(
        &self,
        child_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, Option<ThreadSpawnEdgeStatus>>;

    /// Return whether the incoming edge for `thread_id` and every persisted ancestor edge are
    /// open. Missing incoming edges are not considered an open spawned path.
    fn thread_spawn_path_is_open(&self, thread_id: ThreadId) -> AgentGraphStoreFuture<'_, bool>;

    /// Atomically authorize and reserve closing a strict descendant subtree.
    ///
    /// Authorization and `Open -> Closing` for the target edge occur in one write transaction.
    /// Repeated calls for the same descendant return the existing `Closing`/`Closed` state.
    fn begin_thread_spawn_subtree_close(
        &self,
        root_thread_id: ThreadId,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, Option<ThreadSpawnSubtreeClose>>;

    /// Atomically complete `Closing -> Closed` for a target edge. Already closed targets succeed.
    fn finish_thread_spawn_subtree_close(
        &self,
        target_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, bool>;

    /// Atomically reopen one closed target edge for an authorized V1 resume.
    ///
    /// Implementations must preserve the original parent, reject `Closing`, and require the
    /// caller to be an open ancestor in the same rooted spawn tree.
    fn reopen_closed_thread_spawn_edge(
        &self,
        root_thread_id: ThreadId,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, bool>;

    /// List closing target edges below a root, deepest-first, while traversing every edge status.
    fn list_closing_thread_spawn_targets(
        &self,
        root_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>>;

    /// List direct spawned children of a parent thread.
    ///
    /// When `status_filter` is `Some`, only child edges with that exact status are returned. When
    /// it is `None`, all direct child edges are returned regardless of status, including statuses
    /// that may be added by a future store implementation.
    fn list_thread_spawn_children(
        &self,
        parent_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>>;

    /// List spawned descendants breadth-first by depth, then by thread id.
    ///
    /// `status_filter` is applied to every traversed edge, not just to the returned descendants.
    /// For example, `Some(Open)` walks only open edges, so descendants under a closed edge are not
    /// included even if their own incoming edge is open. `None` walks and returns every persisted
    /// edge regardless of status.
    fn list_thread_spawn_descendants(
        &self,
        root_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>>;

    /// Atomically reserve one of at most two validation submissions for one turn-bound result.
    fn begin_workflow_validation(
        &self,
        submission: WorkflowValidationSubmission,
    ) -> AgentGraphStoreFuture<'_, WorkflowValidationDecision>;

    /// Persist the formal validation and gate details for the exact reserved submission.
    fn complete_workflow_validation(
        &self,
        completion: WorkflowValidationCompletion,
    ) -> AgentGraphStoreFuture<'_, bool>;

    /// Return accepted reviewer evidence for the supplied root turn.
    fn list_workflow_reviewer_validations(
        &self,
        root_thread_id: ThreadId,
        turn_id: &str,
    ) -> AgentGraphStoreFuture<'_, Vec<WorkflowReviewerValidation>>;

    /// Return every workflow validation record for the supplied root turn.
    fn list_workflow_validations(
        &self,
        root_thread_id: ThreadId,
        turn_id: &str,
    ) -> AgentGraphStoreFuture<'_, Vec<WorkflowValidationRecord>>;
}
