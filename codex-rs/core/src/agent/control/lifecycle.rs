use super::*;
use codex_agent_graph_store::ThreadSpawnEdgeStatus;
use std::collections::HashSet;

impl AgentControl {
    pub(super) fn in_memory_spawn_root(&self, thread_id: ThreadId) -> Option<ThreadId> {
        let edges = self
            .spawn_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut current = thread_id;
        let mut visited = HashSet::new();
        while visited.insert(current) {
            let Some(edge) = edges.get(&current) else {
                return (current != thread_id).then_some(current);
            };
            current = edge.parent_thread_id;
        }
        None
    }

    pub(super) fn in_memory_spawn_path_is_open(
        &self,
        root_thread_id: ThreadId,
        thread_id: ThreadId,
    ) -> bool {
        let edges = self
            .spawn_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        spawn_path_is_open(&edges, root_thread_id, thread_id)
    }

    pub(super) fn ensure_in_memory_spawn_edge_open(
        &self,
        root_thread_id: ThreadId,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
    ) -> bool {
        if parent_thread_id == child_thread_id || child_thread_id == root_thread_id {
            return false;
        }
        let mut edges = self
            .spawn_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !spawn_path_is_open(&edges, root_thread_id, parent_thread_id) {
            return false;
        }
        if let Some(edge) = edges.get(&child_thread_id) {
            return edge.parent_thread_id == parent_thread_id
                && edge.status == ThreadSpawnEdgeStatus::Open;
        }
        let mut current = parent_thread_id;
        let mut visited = HashSet::new();
        while current != root_thread_id {
            if current == child_thread_id || !visited.insert(current) {
                return false;
            }
            let Some(edge) = edges.get(&current) else {
                return false;
            };
            current = edge.parent_thread_id;
        }
        edges.insert(
            child_thread_id,
            InMemorySpawnEdge {
                parent_thread_id,
                status: ThreadSpawnEdgeStatus::Open,
            },
        );
        true
    }

    fn begin_in_memory_subtree_close(
        &self,
        root_thread_id: ThreadId,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> Option<(ThreadSpawnEdgeStatus, Vec<ThreadId>)> {
        if caller_thread_id == target_thread_id || target_thread_id == root_thread_id {
            return None;
        }
        let mut edges = self
            .spawn_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !spawn_path_is_open(&edges, root_thread_id, caller_thread_id)
            || !target_is_authorized_descendant(&edges, caller_thread_id, target_thread_id)
        {
            return None;
        }
        let previous_status = edges.get(&target_thread_id)?.status;
        if previous_status == ThreadSpawnEdgeStatus::Open
            && let Some(edge) = edges.get_mut(&target_thread_id)
        {
            edge.status = ThreadSpawnEdgeStatus::Closing;
        }
        let thread_ids_deepest_first = subtree_thread_ids_deepest_first(&edges, target_thread_id);
        Some((previous_status, thread_ids_deepest_first))
    }

    fn set_in_memory_edge_status(&self, thread_id: ThreadId, status: ThreadSpawnEdgeStatus) {
        if let Some(edge) = self
            .spawn_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&thread_id)
        {
            edge.status = status;
        }
    }

    fn reopen_in_memory_spawn_edge(
        &self,
        root_thread_id: ThreadId,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> bool {
        if caller_thread_id == target_thread_id || target_thread_id == root_thread_id {
            return false;
        }
        let mut edges = self
            .spawn_edges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !spawn_path_is_open(&edges, root_thread_id, caller_thread_id)
            || !target_is_authorized_descendant(&edges, caller_thread_id, target_thread_id)
        {
            return false;
        }
        let Some(edge) = edges.get_mut(&target_thread_id) else {
            return false;
        };
        match edge.status {
            ThreadSpawnEdgeStatus::Open => true,
            ThreadSpawnEdgeStatus::Closing => false,
            ThreadSpawnEdgeStatus::Closed => {
                edge.status = ThreadSpawnEdgeStatus::Open;
                true
            }
        }
    }

    /// Atomically authorize and close a spawned subtree, releasing descendants deepest-first.
    pub(crate) async fn close_agent(
        &self,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> CodexResult<CloseAgentOutcome> {
        let _operation_guard = Arc::clone(&self.close_gate).lock_owned().await;
        let state = self.upgrade()?;
        let root_thread_id = self.root_thread_id_for(caller_thread_id).await;
        let lifecycle_guard = Arc::clone(&self.lifecycle_gate).write_owned().await;

        let mut persisted_close = false;
        let mut thread_ids_deepest_first = Vec::new();
        if let Some(agent_graph_store) = state.agent_graph_store() {
            let close = agent_graph_store
                .begin_thread_spawn_subtree_close(
                    root_thread_id,
                    caller_thread_id,
                    target_thread_id,
                )
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to reserve closing agent `{target_thread_id}`: {error}"
                    ))
                })?;
            if let Some(close) = close {
                persisted_close = true;
                thread_ids_deepest_first = close.thread_ids_deepest_first;
                self.set_in_memory_edge_status(target_thread_id, ThreadSpawnEdgeStatus::Closing);
            } else {
                let persisted_status = agent_graph_store
                    .thread_spawn_edge_status(target_thread_id)
                    .await
                    .map_err(|error| {
                        CodexErr::Fatal(format!(
                            "failed to read lifecycle for agent `{target_thread_id}`: {error}"
                        ))
                    })?;
                let target_is_in_memory = self.in_memory_spawn_root(target_thread_id).is_some();
                let target_is_ephemeral = match state.get_thread(target_thread_id).await {
                    Ok(thread) => thread.config_snapshot().await.ephemeral,
                    Err(_) => false,
                };
                if persisted_status.is_some() || (!target_is_in_memory && !target_is_ephemeral) {
                    return Err(close_not_authorized(caller_thread_id, target_thread_id));
                }
            }
        }
        if !persisted_close {
            let Some((_, in_memory_ids)) = self.begin_in_memory_subtree_close(
                root_thread_id,
                caller_thread_id,
                target_thread_id,
            ) else {
                return Err(close_not_authorized(caller_thread_id, target_thread_id));
            };
            thread_ids_deepest_first = in_memory_ids;
        }

        let previous_status = self.get_status(target_thread_id).await;
        let live_thread_ids = self
            .list_live_agent_subtree_thread_ids(target_thread_id)
            .await?;
        for live_thread_id in live_thread_ids.into_iter().rev() {
            if !thread_ids_deepest_first.contains(&live_thread_id) {
                let target_position = thread_ids_deepest_first
                    .iter()
                    .position(|thread_id| *thread_id == target_thread_id)
                    .unwrap_or(thread_ids_deepest_first.len());
                thread_ids_deepest_first.insert(target_position, live_thread_id);
            }
        }
        drop(lifecycle_guard);

        let mut shutdown_errors = Vec::new();
        for thread_id in thread_ids_deepest_first.iter().copied() {
            if let Err(error) = self.shutdown_live_agent_bounded(thread_id).await {
                shutdown_errors.push(format!("{thread_id}: {error}"));
            }
        }
        if !shutdown_errors.is_empty() {
            return Err(CodexErr::Fatal(format!(
                "failed to close agent subtree `{target_thread_id}`: {}",
                shutdown_errors.join("; ")
            )));
        }

        if persisted_close {
            let Some(agent_graph_store) = state.agent_graph_store() else {
                return Err(CodexErr::Fatal(
                    "agent graph store disappeared while closing a subtree".to_string(),
                ));
            };
            if !agent_graph_store
                .finish_thread_spawn_subtree_close(target_thread_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to finish closing agent `{target_thread_id}`: {error}"
                    ))
                })?
            {
                return Err(CodexErr::Fatal(format!(
                    "agent `{target_thread_id}` did not remain in the closing state"
                )));
            }
        }
        let _lifecycle_guard = self.lifecycle_gate.write().await;
        self.set_in_memory_edge_status(target_thread_id, ThreadSpawnEdgeStatus::Closed);
        for thread_id in thread_ids_deepest_first {
            self.remove_in_memory_workflow_agent(thread_id);
        }
        Ok(CloseAgentOutcome { previous_status })
    }

    /// Finish persisted close operations interrupted by a prior process before restoring agents.
    pub(crate) async fn reconcile_closing_subtrees(
        &self,
        root_thread_id: ThreadId,
    ) -> CodexResult<()> {
        let _operation_guard = Arc::clone(&self.close_gate).lock_owned().await;
        let state = self.upgrade()?;
        let Some(agent_graph_store) = state.agent_graph_store() else {
            return Ok(());
        };
        let closing_targets = agent_graph_store
            .list_closing_thread_spawn_targets(root_thread_id)
            .await
            .map_err(|error| {
                CodexErr::Fatal(format!(
                    "failed to list interrupted close operations for `{root_thread_id}`: {error}"
                ))
            })?;
        for target_thread_id in closing_targets {
            let mut thread_ids = agent_graph_store
                .list_thread_spawn_descendants(target_thread_id, /*status_filter*/ None)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to load closing subtree `{target_thread_id}`: {error}"
                    ))
                })?;
            thread_ids.reverse();
            thread_ids.push(target_thread_id);
            let mut shutdown_errors = Vec::new();
            for thread_id in thread_ids {
                if let Err(error) = self.shutdown_live_agent_bounded(thread_id).await {
                    shutdown_errors.push(format!("{thread_id}: {error}"));
                }
            }
            if !shutdown_errors.is_empty() {
                return Err(CodexErr::Fatal(format!(
                    "failed to reconcile closing subtree `{target_thread_id}`: {}",
                    shutdown_errors.join("; ")
                )));
            }
            if !agent_graph_store
                .finish_thread_spawn_subtree_close(target_thread_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to finish reconciled close `{target_thread_id}`: {error}"
                    ))
                })?
            {
                return Err(CodexErr::Fatal(format!(
                    "reconciled agent `{target_thread_id}` did not remain closing"
                )));
            }
        }
        Ok(())
    }

    /// Authorize a V1 resume and reopen exactly one previously closed target edge.
    pub(crate) async fn reopen_agent_for_resume(
        &self,
        caller_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> CodexResult<()> {
        let _operation_guard = Arc::clone(&self.close_gate).lock_owned().await;
        let state = self.upgrade()?;
        let root_thread_id = self.root_thread_id_for(caller_thread_id).await;
        let _lifecycle_guard = Arc::clone(&self.lifecycle_gate).write_owned().await;
        let reopened = if let Some(agent_graph_store) = state.agent_graph_store()
            && agent_graph_store
                .thread_spawn_edge_status(target_thread_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to read lifecycle for agent `{target_thread_id}`: {error}"
                    ))
                })?
                .is_some()
        {
            agent_graph_store
                .reopen_closed_thread_spawn_edge(root_thread_id, caller_thread_id, target_thread_id)
                .await
                .map_err(|error| {
                    CodexErr::Fatal(format!(
                        "failed to reopen agent `{target_thread_id}`: {error}"
                    ))
                })?
        } else {
            self.reopen_in_memory_spawn_edge(root_thread_id, caller_thread_id, target_thread_id)
        };
        if !reopened {
            return Err(CodexErr::UnsupportedOperation(format!(
                "agent `{target_thread_id}` is not a resumable descendant of caller `{caller_thread_id}`"
            )));
        }
        self.set_in_memory_edge_status(target_thread_id, ThreadSpawnEdgeStatus::Open);
        Ok(())
    }
}

fn spawn_path_is_open(
    edges: &HashMap<ThreadId, InMemorySpawnEdge>,
    root_thread_id: ThreadId,
    thread_id: ThreadId,
) -> bool {
    if edges.contains_key(&root_thread_id) {
        return false;
    }
    if thread_id == root_thread_id {
        return true;
    }
    let mut current = thread_id;
    let mut visited = HashSet::new();
    while current != root_thread_id {
        if !visited.insert(current) {
            return false;
        }
        let Some(edge) = edges.get(&current) else {
            return false;
        };
        if edge.status != ThreadSpawnEdgeStatus::Open {
            return false;
        }
        current = edge.parent_thread_id;
    }
    true
}

fn target_is_authorized_descendant(
    edges: &HashMap<ThreadId, InMemorySpawnEdge>,
    caller_thread_id: ThreadId,
    target_thread_id: ThreadId,
) -> bool {
    let mut current = target_thread_id;
    let mut target_edge = true;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(current) {
            return false;
        }
        let Some(edge) = edges.get(&current) else {
            return false;
        };
        if !target_edge && edge.status != ThreadSpawnEdgeStatus::Open {
            return false;
        }
        if edge.parent_thread_id == caller_thread_id {
            return true;
        }
        current = edge.parent_thread_id;
        target_edge = false;
    }
}

fn subtree_thread_ids_deepest_first(
    edges: &HashMap<ThreadId, InMemorySpawnEdge>,
    target_thread_id: ThreadId,
) -> Vec<ThreadId> {
    let mut descendants = Vec::new();
    for thread_id in edges.keys().copied() {
        let mut current = thread_id;
        let mut depth = 0usize;
        let mut visited = HashSet::new();
        while current != target_thread_id {
            if !visited.insert(current) {
                break;
            }
            let Some(edge) = edges.get(&current) else {
                break;
            };
            current = edge.parent_thread_id;
            depth += 1;
        }
        if current == target_thread_id {
            descendants.push((depth, thread_id));
        }
    }
    descendants.push((0, target_thread_id));
    descendants.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.to_string().cmp(&right.1.to_string()))
    });
    descendants.dedup_by_key(|(_, thread_id)| *thread_id);
    descendants
        .into_iter()
        .map(|(_, thread_id)| thread_id)
        .collect()
}

fn close_not_authorized(caller_thread_id: ThreadId, target_thread_id: ThreadId) -> CodexErr {
    CodexErr::UnsupportedOperation(format!(
        "agent `{target_thread_id}` is not a descendant of caller `{caller_thread_id}`"
    ))
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
