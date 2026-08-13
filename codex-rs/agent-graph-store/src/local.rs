use codex_protocol::ThreadId;
use codex_state::StateRuntime;
use std::sync::Arc;

use crate::AgentGraphStore;
use crate::AgentGraphStoreError;
use crate::AgentGraphStoreFuture;
use crate::ThreadSpawnEdgeStatus;
use crate::ThreadSpawnSubtreeClose;
use crate::WorkflowReviewerValidation;
use crate::WorkflowValidationCompletion;
use crate::WorkflowValidationDecision;
use crate::WorkflowValidationRecord;
use crate::WorkflowValidationSubmission;

/// SQLite-backed implementation of [`AgentGraphStore`] using an existing state runtime.
#[derive(Clone)]
pub struct LocalAgentGraphStore {
    state_db: Arc<StateRuntime>,
}

impl std::fmt::Debug for LocalAgentGraphStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalAgentGraphStore")
            .field("sqlite", self.state_db.sqlite())
            .finish_non_exhaustive()
    }
}

impl LocalAgentGraphStore {
    /// Create a local graph store from an already-initialized state runtime.
    pub fn new(state_db: Arc<StateRuntime>) -> Self {
        Self { state_db }
    }
}

impl AgentGraphStore for LocalAgentGraphStore {
    fn upsert_thread_spawn_edge(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        status: ThreadSpawnEdgeStatus,
    ) -> AgentGraphStoreFuture<'_, ()> {
        Box::pin(async move {
            self.state_db
                .upsert_thread_spawn_edge(
                    parent_thread_id,
                    child_thread_id,
                    to_state_status(status),
                )
                .await
                .map_err(internal_error)
        })
    }

    fn set_thread_spawn_edge_status(
        &self,
        child_thread_id: ThreadId,
        status: ThreadSpawnEdgeStatus,
    ) -> AgentGraphStoreFuture<'_, ()> {
        Box::pin(async move {
            self.state_db
                .set_thread_spawn_edge_status(child_thread_id, to_state_status(status))
                .await
                .map_err(internal_error)
        })
    }

    fn ensure_thread_spawn_edge_open(
        &self,
        root_thread_id: ThreadId,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, bool> {
        Box::pin(async move {
            self.state_db
                .ensure_thread_spawn_edge_open(root_thread_id, parent_thread_id, child_thread_id)
                .await
                .map_err(internal_error)
        })
    }

    fn thread_spawn_edge_status(
        &self,
        child_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, Option<ThreadSpawnEdgeStatus>> {
        Box::pin(async move {
            self.state_db
                .thread_spawn_edge_status(child_thread_id)
                .await
                .map(|status| status.map(from_state_status))
                .map_err(internal_error)
        })
    }

    fn thread_spawn_path_is_open(&self, thread_id: ThreadId) -> AgentGraphStoreFuture<'_, bool> {
        Box::pin(async move {
            self.state_db
                .thread_spawn_path_is_open(thread_id)
                .await
                .map_err(internal_error)
        })
    }

    fn begin_thread_spawn_subtree_close(
        &self,
        root_thread_id: ThreadId,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, Option<ThreadSpawnSubtreeClose>> {
        Box::pin(async move {
            self.state_db
                .begin_thread_spawn_subtree_close(
                    root_thread_id,
                    caller_thread_id,
                    target_thread_id,
                )
                .await
                .map(|reservation| {
                    reservation.map(|(previous_edge_status, thread_ids_deepest_first)| {
                        ThreadSpawnSubtreeClose {
                            previous_edge_status: from_state_status(previous_edge_status),
                            thread_ids_deepest_first,
                        }
                    })
                })
                .map_err(internal_error)
        })
    }

    fn finish_thread_spawn_subtree_close(
        &self,
        target_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, bool> {
        Box::pin(async move {
            self.state_db
                .finish_thread_spawn_subtree_close(target_thread_id)
                .await
                .map_err(internal_error)
        })
    }

    fn reopen_closed_thread_spawn_edge(
        &self,
        root_thread_id: ThreadId,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, bool> {
        Box::pin(async move {
            self.state_db
                .reopen_closed_thread_spawn_edge(root_thread_id, caller_thread_id, target_thread_id)
                .await
                .map_err(internal_error)
        })
    }

    fn list_closing_thread_spawn_targets(
        &self,
        root_thread_id: ThreadId,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            self.state_db
                .list_closing_thread_spawn_targets(root_thread_id)
                .await
                .map_err(internal_error)
        })
    }

    fn list_thread_spawn_children(
        &self,
        parent_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            if let Some(status) = status_filter {
                return self
                    .state_db
                    .list_thread_spawn_children_with_status(
                        parent_thread_id,
                        to_state_status(status),
                    )
                    .await
                    .map_err(internal_error);
            }

            self.state_db
                .list_thread_spawn_children(parent_thread_id)
                .await
                .map_err(internal_error)
        })
    }

    fn list_thread_spawn_descendants(
        &self,
        root_thread_id: ThreadId,
        status_filter: Option<ThreadSpawnEdgeStatus>,
    ) -> AgentGraphStoreFuture<'_, Vec<ThreadId>> {
        Box::pin(async move {
            match status_filter {
                Some(status) => self
                    .state_db
                    .list_thread_spawn_descendants_with_status(
                        root_thread_id,
                        to_state_status(status),
                    )
                    .await
                    .map_err(internal_error),
                None => self
                    .state_db
                    .list_thread_spawn_descendants(root_thread_id)
                    .await
                    .map_err(internal_error),
            }
        })
    }

    fn begin_workflow_validation(
        &self,
        submission: WorkflowValidationSubmission,
    ) -> AgentGraphStoreFuture<'_, WorkflowValidationDecision> {
        Box::pin(async move {
            self.state_db
                .begin_workflow_validation(to_state_submission(submission))
                .await
                .map(from_state_decision)
                .map_err(internal_error)
        })
    }

    fn complete_workflow_validation(
        &self,
        completion: WorkflowValidationCompletion,
    ) -> AgentGraphStoreFuture<'_, bool> {
        Box::pin(async move {
            self.state_db
                .complete_workflow_validation(to_state_completion(completion))
                .await
                .map_err(internal_error)
        })
    }

    fn list_workflow_reviewer_validations(
        &self,
        root_thread_id: ThreadId,
        turn_id: &str,
    ) -> AgentGraphStoreFuture<'_, Vec<WorkflowReviewerValidation>> {
        let turn_id = turn_id.to_string();
        Box::pin(async move {
            self.state_db
                .list_workflow_reviewer_validations(root_thread_id, &turn_id)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|record| WorkflowReviewerValidation {
                            agent_thread_id: record.agent_thread_id,
                            role: record.role,
                            output_revision: record.output_revision,
                            output_sha256: record.output_sha256,
                            reviewer_pass: record.reviewer_pass,
                            has_high_or_critical: record.has_high_or_critical,
                        })
                        .collect()
                })
                .map_err(internal_error)
        })
    }

    fn list_workflow_validations(
        &self,
        root_thread_id: ThreadId,
        turn_id: &str,
    ) -> AgentGraphStoreFuture<'_, Vec<WorkflowValidationRecord>> {
        let turn_id = turn_id.to_string();
        Box::pin(async move {
            self.state_db
                .list_workflow_validations(root_thread_id, &turn_id)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(|record| WorkflowValidationRecord {
                            validation_key: record.validation_key,
                            result_kind: record.result_kind,
                            agent_thread_id: record.agent_thread_id,
                            agent_role: record.agent_role,
                            submissions: record.submissions,
                            accepted: record.accepted,
                            output_revision: record.output_revision,
                            output_sha256: record.output_sha256,
                            reviewer_pass: record.reviewer_pass,
                            has_high_or_critical: record.has_high_or_critical,
                        })
                        .collect()
                })
                .map_err(internal_error)
        })
    }
}

fn to_state_submission(
    submission: WorkflowValidationSubmission,
) -> codex_state::WorkflowValidationSubmission {
    codex_state::WorkflowValidationSubmission {
        root_thread_id: submission.root_thread_id,
        turn_id: submission.turn_id,
        validation_key: submission.validation_key,
        result_kind: submission.result_kind,
        agent_thread_id: submission.agent_thread_id,
        reviewer_role: submission.reviewer_role,
        output_revision: submission.output_revision,
        output_sha256: submission.output_sha256,
        requires_higher_revision: submission.requires_higher_revision,
    }
}

fn from_state_decision(
    decision: codex_state::WorkflowValidationDecision,
) -> WorkflowValidationDecision {
    match decision {
        codex_state::WorkflowValidationDecision::Attempt(submission) => {
            WorkflowValidationDecision::Attempt(submission)
        }
        codex_state::WorkflowValidationDecision::AlreadyAccepted(submissions) => {
            WorkflowValidationDecision::AlreadyAccepted(submissions)
        }
        codex_state::WorkflowValidationDecision::Exhausted(submissions) => {
            WorkflowValidationDecision::Exhausted(submissions)
        }
        codex_state::WorkflowValidationDecision::UnchangedCorrection(submissions) => {
            WorkflowValidationDecision::UnchangedCorrection(submissions)
        }
        codex_state::WorkflowValidationDecision::ResultLimitReached => {
            WorkflowValidationDecision::ResultLimitReached
        }
    }
}

fn to_state_completion(
    completion: WorkflowValidationCompletion,
) -> codex_state::WorkflowValidationCompletion {
    codex_state::WorkflowValidationCompletion {
        root_thread_id: completion.root_thread_id,
        turn_id: completion.turn_id,
        validation_key: completion.validation_key,
        submission: completion.submission,
        output_revision: completion.output_revision,
        output_sha256: completion.output_sha256,
        reviewer_pass: completion.reviewer_pass,
        has_high_or_critical: completion.has_high_or_critical,
    }
}

fn to_state_status(status: ThreadSpawnEdgeStatus) -> codex_state::DirectionalThreadSpawnEdgeStatus {
    match status {
        ThreadSpawnEdgeStatus::Open => codex_state::DirectionalThreadSpawnEdgeStatus::Open,
        ThreadSpawnEdgeStatus::Closing => codex_state::DirectionalThreadSpawnEdgeStatus::Closing,
        ThreadSpawnEdgeStatus::Closed => codex_state::DirectionalThreadSpawnEdgeStatus::Closed,
    }
}

fn from_state_status(
    status: codex_state::DirectionalThreadSpawnEdgeStatus,
) -> ThreadSpawnEdgeStatus {
    match status {
        codex_state::DirectionalThreadSpawnEdgeStatus::Open => ThreadSpawnEdgeStatus::Open,
        codex_state::DirectionalThreadSpawnEdgeStatus::Closing => ThreadSpawnEdgeStatus::Closing,
        codex_state::DirectionalThreadSpawnEdgeStatus::Closed => ThreadSpawnEdgeStatus::Closed,
    }
}

fn internal_error(err: impl std::fmt::Display) -> AgentGraphStoreError {
    AgentGraphStoreError::Internal {
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_state::DirectionalThreadSpawnEdgeStatus;
    use codex_utils_absolute_path::test_support::PathExt;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    struct TestRuntime {
        state_db: Arc<StateRuntime>,
        _codex_home: TempDir,
    }

    fn thread_id(suffix: u128) -> ThreadId {
        ThreadId::from_string(&format!("00000000-0000-0000-0000-{suffix:012}"))
            .expect("valid thread id")
    }

    async fn state_runtime() -> TestRuntime {
        let codex_home = TempDir::new().expect("tempdir should be created");
        let state_db = StateRuntime::init(
            codex_state::SqliteConfig::new_for_testing(codex_home.path().abs()),
            "test-provider".to_string(),
        )
        .await
        .expect("state db should initialize");
        TestRuntime {
            state_db,
            _codex_home: codex_home,
        }
    }

    #[tokio::test]
    async fn local_store_upserts_and_lists_direct_children_with_status_filters() {
        let fixture = state_runtime().await;
        let state_db = fixture.state_db;
        let store = LocalAgentGraphStore::new(state_db.clone());
        let parent_thread_id = thread_id(/*suffix*/ 1);
        let first_child_thread_id = thread_id(/*suffix*/ 2);
        let second_child_thread_id = thread_id(/*suffix*/ 3);

        store
            .upsert_thread_spawn_edge(
                parent_thread_id,
                second_child_thread_id,
                ThreadSpawnEdgeStatus::Closed,
            )
            .await
            .expect("closed child edge should insert");
        store
            .upsert_thread_spawn_edge(
                parent_thread_id,
                first_child_thread_id,
                ThreadSpawnEdgeStatus::Open,
            )
            .await
            .expect("open child edge should insert");

        let all_children = store
            .list_thread_spawn_children(parent_thread_id, /*status_filter*/ None)
            .await
            .expect("all children should load");
        assert_eq!(
            all_children,
            vec![first_child_thread_id, second_child_thread_id]
        );

        let open_children = store
            .list_thread_spawn_children(parent_thread_id, Some(ThreadSpawnEdgeStatus::Open))
            .await
            .expect("open children should load");
        let state_open_children = state_db
            .list_thread_spawn_children_with_status(
                parent_thread_id,
                DirectionalThreadSpawnEdgeStatus::Open,
            )
            .await
            .expect("state open children should load");
        assert_eq!(open_children, state_open_children);
        assert_eq!(open_children, vec![first_child_thread_id]);

        let closed_children = store
            .list_thread_spawn_children(parent_thread_id, Some(ThreadSpawnEdgeStatus::Closed))
            .await
            .expect("closed children should load");
        assert_eq!(closed_children, vec![second_child_thread_id]);
    }

    #[tokio::test]
    async fn local_store_updates_edge_status() {
        let fixture = state_runtime().await;
        let state_db = fixture.state_db;
        let store = LocalAgentGraphStore::new(state_db);
        let parent_thread_id = thread_id(/*suffix*/ 10);
        let child_thread_id = thread_id(/*suffix*/ 11);

        store
            .upsert_thread_spawn_edge(
                parent_thread_id,
                child_thread_id,
                ThreadSpawnEdgeStatus::Open,
            )
            .await
            .expect("child edge should insert");
        store
            .set_thread_spawn_edge_status(child_thread_id, ThreadSpawnEdgeStatus::Closed)
            .await
            .expect("child edge should close");

        let open_children = store
            .list_thread_spawn_children(parent_thread_id, Some(ThreadSpawnEdgeStatus::Open))
            .await
            .expect("open children should load");
        assert_eq!(open_children, Vec::<ThreadId>::new());

        let closed_children = store
            .list_thread_spawn_children(parent_thread_id, Some(ThreadSpawnEdgeStatus::Closed))
            .await
            .expect("closed children should load");
        assert_eq!(closed_children, vec![child_thread_id]);
    }

    #[tokio::test]
    async fn local_store_lists_descendants_breadth_first_with_status_filters() {
        let fixture = state_runtime().await;
        let state_db = fixture.state_db;
        let store = LocalAgentGraphStore::new(state_db.clone());
        let root_thread_id = thread_id(/*suffix*/ 20);
        let later_child_thread_id = thread_id(/*suffix*/ 22);
        let earlier_child_thread_id = thread_id(/*suffix*/ 21);
        let closed_grandchild_thread_id = thread_id(/*suffix*/ 23);
        let open_grandchild_thread_id = thread_id(/*suffix*/ 24);
        let closed_child_thread_id = thread_id(/*suffix*/ 25);
        let closed_great_grandchild_thread_id = thread_id(/*suffix*/ 26);

        for (parent_thread_id, child_thread_id, status) in [
            (
                root_thread_id,
                later_child_thread_id,
                ThreadSpawnEdgeStatus::Open,
            ),
            (
                root_thread_id,
                earlier_child_thread_id,
                ThreadSpawnEdgeStatus::Open,
            ),
            (
                earlier_child_thread_id,
                open_grandchild_thread_id,
                ThreadSpawnEdgeStatus::Open,
            ),
            (
                later_child_thread_id,
                closed_grandchild_thread_id,
                ThreadSpawnEdgeStatus::Closed,
            ),
            (
                root_thread_id,
                closed_child_thread_id,
                ThreadSpawnEdgeStatus::Closed,
            ),
            (
                closed_child_thread_id,
                closed_great_grandchild_thread_id,
                ThreadSpawnEdgeStatus::Closed,
            ),
        ] {
            store
                .upsert_thread_spawn_edge(parent_thread_id, child_thread_id, status)
                .await
                .expect("edge should insert");
        }

        let all_descendants = store
            .list_thread_spawn_descendants(root_thread_id, /*status_filter*/ None)
            .await
            .expect("all descendants should load");
        assert_eq!(
            all_descendants,
            vec![
                earlier_child_thread_id,
                later_child_thread_id,
                closed_child_thread_id,
                closed_grandchild_thread_id,
                open_grandchild_thread_id,
                closed_great_grandchild_thread_id,
            ]
        );

        let open_descendants = store
            .list_thread_spawn_descendants(root_thread_id, Some(ThreadSpawnEdgeStatus::Open))
            .await
            .expect("open descendants should load");
        let state_open_descendants = state_db
            .list_thread_spawn_descendants_with_status(
                root_thread_id,
                DirectionalThreadSpawnEdgeStatus::Open,
            )
            .await
            .expect("state open descendants should load");
        assert_eq!(open_descendants, state_open_descendants);
        assert_eq!(
            open_descendants,
            vec![
                earlier_child_thread_id,
                later_child_thread_id,
                open_grandchild_thread_id,
            ]
        );

        let closed_descendants = store
            .list_thread_spawn_descendants(root_thread_id, Some(ThreadSpawnEdgeStatus::Closed))
            .await
            .expect("closed descendants should load");
        assert_eq!(
            closed_descendants,
            vec![closed_child_thread_id, closed_great_grandchild_thread_id]
        );
    }

    #[tokio::test]
    async fn close_saga_authorizes_once_and_changes_only_the_target_edge() {
        let fixture = state_runtime().await;
        let store = LocalAgentGraphStore::new(fixture.state_db);
        let root = thread_id(/*suffix*/ 100);
        let child = thread_id(/*suffix*/ 101);
        let sibling = thread_id(/*suffix*/ 102);
        let grandchild = thread_id(/*suffix*/ 103);
        let leaf = thread_id(/*suffix*/ 104);
        for (parent, target) in [
            (root, child),
            (root, sibling),
            (child, grandchild),
            (grandchild, leaf),
        ] {
            assert!(
                store
                    .ensure_thread_spawn_edge_open(root, parent, target)
                    .await
                    .expect("open edge should persist")
            );
        }
        store
            .set_thread_spawn_edge_status(leaf, ThreadSpawnEdgeStatus::Closed)
            .await
            .expect("leaf should close independently");

        let close = store
            .begin_thread_spawn_subtree_close(root, child, grandchild)
            .await
            .expect("close reservation should succeed")
            .expect("child should authorize its descendant");
        assert_eq!(
            close,
            ThreadSpawnSubtreeClose {
                previous_edge_status: ThreadSpawnEdgeStatus::Open,
                thread_ids_deepest_first: vec![leaf, grandchild],
            }
        );
        assert_eq!(
            store
                .thread_spawn_edge_status(grandchild)
                .await
                .expect("target status should load"),
            Some(ThreadSpawnEdgeStatus::Closing)
        );
        assert_eq!(
            store
                .thread_spawn_edge_status(leaf)
                .await
                .expect("leaf status should load"),
            Some(ThreadSpawnEdgeStatus::Closed)
        );
        assert!(
            store
                .finish_thread_spawn_subtree_close(grandchild)
                .await
                .expect("close should finish")
        );

        assert_eq!(
            store
                .begin_thread_spawn_subtree_close(root, sibling, grandchild)
                .await
                .expect("wrong-caller check should complete"),
            None
        );
        let repeated = store
            .begin_thread_spawn_subtree_close(root, child, grandchild)
            .await
            .expect("authorized retry should complete")
            .expect("same caller remains authorized");
        assert_eq!(repeated.previous_edge_status, ThreadSpawnEdgeStatus::Closed);
        assert!(
            store
                .reopen_closed_thread_spawn_edge(root, child, grandchild)
                .await
                .expect("authorized resume should complete")
        );
        assert_eq!(
            store
                .thread_spawn_edge_status(grandchild)
                .await
                .expect("reopened target status should load"),
            Some(ThreadSpawnEdgeStatus::Open)
        );
        assert_eq!(
            store
                .thread_spawn_edge_status(leaf)
                .await
                .expect("independently closed leaf status should load"),
            Some(ThreadSpawnEdgeStatus::Closed)
        );
    }

    #[tokio::test]
    async fn closing_ancestor_blocks_existing_child_new_spawn_reparent_and_cycle() {
        let fixture = state_runtime().await;
        let store = LocalAgentGraphStore::new(fixture.state_db);
        let root = thread_id(/*suffix*/ 110);
        let child = thread_id(/*suffix*/ 111);
        let grandchild = thread_id(/*suffix*/ 112);
        let future_child = thread_id(/*suffix*/ 113);
        assert!(
            store
                .ensure_thread_spawn_edge_open(root, root, child)
                .await
                .expect("root child should persist")
        );
        assert!(
            store
                .ensure_thread_spawn_edge_open(root, child, grandchild)
                .await
                .expect("grandchild should persist")
        );
        store
            .begin_thread_spawn_subtree_close(root, root, child)
            .await
            .expect("root close should complete")
            .expect("root should authorize child close");

        assert!(
            !store
                .ensure_thread_spawn_edge_open(root, child, grandchild)
                .await
                .expect("existing edge check should complete")
        );
        assert!(
            !store
                .ensure_thread_spawn_edge_open(root, grandchild, future_child)
                .await
                .expect("new edge check should complete")
        );
        assert!(
            !store
                .ensure_thread_spawn_edge_open(root, root, grandchild)
                .await
                .expect("reparent check should complete")
        );
        assert!(
            !store
                .ensure_thread_spawn_edge_open(root, grandchild, root)
                .await
                .expect("cycle check should complete")
        );
    }

    #[tokio::test]
    async fn closing_targets_are_found_below_open_edges_deepest_first() {
        let fixture = state_runtime().await;
        let store = LocalAgentGraphStore::new(fixture.state_db);
        let root = thread_id(/*suffix*/ 120);
        let child = thread_id(/*suffix*/ 121);
        let grandchild = thread_id(/*suffix*/ 122);
        let leaf = thread_id(/*suffix*/ 123);
        for (parent, target, status) in [
            (root, child, ThreadSpawnEdgeStatus::Closing),
            (child, grandchild, ThreadSpawnEdgeStatus::Open),
            (grandchild, leaf, ThreadSpawnEdgeStatus::Closing),
        ] {
            store
                .upsert_thread_spawn_edge(parent, target, status)
                .await
                .expect("test edge should persist");
        }

        assert_eq!(
            store
                .list_closing_thread_spawn_targets(root)
                .await
                .expect("closing targets should load"),
            vec![leaf, child]
        );
    }

    fn validation_submission(
        root_thread_id: ThreadId,
        turn_id: &str,
        validation_key: &str,
        output_revision: u32,
        output_sha256: &str,
    ) -> WorkflowValidationSubmission {
        WorkflowValidationSubmission {
            root_thread_id,
            turn_id: turn_id.to_string(),
            validation_key: validation_key.to_string(),
            result_kind: "reviewer".to_string(),
            agent_thread_id: Some(thread_id(/*suffix*/ 902)),
            reviewer_role: Some("Security".to_string()),
            output_revision,
            output_sha256: output_sha256.to_string(),
            requires_higher_revision: true,
        }
    }

    #[tokio::test]
    async fn workflow_validation_is_turn_bound_persistent_and_requires_new_revision() {
        let fixture = state_runtime().await;
        let store = LocalAgentGraphStore::new(fixture.state_db.clone());
        let reopened_store = LocalAgentGraphStore::new(fixture.state_db);
        let root = thread_id(/*suffix*/ 900);
        let first = validation_submission(root, "turn-1", "reviewer:security", 1, &"a".repeat(64));
        assert_eq!(
            store
                .begin_workflow_validation(first.clone())
                .await
                .expect("first validation should reserve"),
            WorkflowValidationDecision::Attempt(1)
        );
        assert_eq!(
            reopened_store
                .begin_workflow_validation(first)
                .await
                .expect("resumed validation state should load"),
            WorkflowValidationDecision::UnchangedCorrection(2)
        );
        assert_eq!(
            store
                .begin_workflow_validation(validation_submission(
                    root,
                    "turn-1",
                    "reviewer:security",
                    2,
                    &"b".repeat(64),
                ))
                .await
                .expect("exhausted validation should load"),
            WorkflowValidationDecision::Exhausted(2)
        );
        assert_eq!(
            store
                .begin_workflow_validation(validation_submission(
                    root,
                    "turn-2",
                    "reviewer:security",
                    1,
                    &"a".repeat(64),
                ))
                .await
                .expect("new turn should be isolated"),
            WorkflowValidationDecision::Attempt(1)
        );
    }

    #[tokio::test]
    async fn reviewer_evidence_records_distinct_pass_and_allows_one_fix_cycle() {
        let fixture = state_runtime().await;
        let store = LocalAgentGraphStore::new(fixture.state_db);
        let root = thread_id(/*suffix*/ 910);
        let child = thread_id(/*suffix*/ 902);
        let first =
            validation_submission(root, "turn-review", "reviewer:security", 1, &"c".repeat(64));
        assert_eq!(
            store
                .begin_workflow_validation(first)
                .await
                .expect("first review should reserve"),
            WorkflowValidationDecision::Attempt(1)
        );
        assert!(
            store
                .complete_workflow_validation(WorkflowValidationCompletion {
                    root_thread_id: root,
                    turn_id: "turn-review".to_string(),
                    validation_key: "reviewer:security".to_string(),
                    submission: 1,
                    output_revision: 1,
                    output_sha256: "c".repeat(64),
                    reviewer_pass: false,
                    has_high_or_critical: true,
                })
                .await
                .expect("review completion should persist")
        );
        assert_eq!(
            store
                .begin_workflow_validation(validation_submission(
                    root,
                    "turn-review",
                    "reviewer:security",
                    2,
                    &"d".repeat(64),
                ))
                .await
                .expect("fixed review should reserve"),
            WorkflowValidationDecision::Attempt(1)
        );
        assert!(
            store
                .complete_workflow_validation(WorkflowValidationCompletion {
                    root_thread_id: root,
                    turn_id: "turn-review".to_string(),
                    validation_key: "reviewer:security".to_string(),
                    submission: 1,
                    output_revision: 2,
                    output_sha256: "d".repeat(64),
                    reviewer_pass: true,
                    has_high_or_critical: false,
                })
                .await
                .expect("fixed review completion should persist")
        );
        assert_eq!(
            store
                .list_workflow_reviewer_validations(root, "turn-review")
                .await
                .expect("review evidence should load"),
            vec![WorkflowReviewerValidation {
                agent_thread_id: child,
                role: "Security".to_string(),
                output_revision: 2,
                output_sha256: "d".repeat(64),
                reviewer_pass: true,
                has_high_or_critical: false,
            }]
        );
    }
}
