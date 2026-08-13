use super::AgentControl;
use crate::agent::AgentStatus;
use crate::codex_thread::CodexThread;
use crate::config::Config;
use crate::thread_manager::ThreadManagerState;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use tracing::warn;

#[derive(Debug, Default)]
pub(super) struct V2Residency {
    state: Mutex<V2ResidencyState>,
}

#[derive(Debug, Default)]
struct V2ResidencyState {
    residents: VecDeque<ThreadId>,
    pending_slots: usize,
    pins: HashMap<ThreadId, usize>,
    evicting: HashSet<ThreadId>,
}

#[derive(Debug)]
pub(crate) struct V2ResidencyPin {
    residency: Arc<V2Residency>,
    thread_id: ThreadId,
}

impl Drop for V2ResidencyPin {
    fn drop(&mut self) {
        self.residency.unpin(self.thread_id);
    }
}

pub(super) struct V2ResidencySlot {
    residency: Arc<V2Residency>,
    active: bool,
}

impl V2ResidencySlot {
    pub(super) fn commit(mut self, thread_id: ThreadId) {
        self.residency.commit_slot(thread_id);
        self.active = false;
    }

    pub(super) fn commit_pinned(mut self, thread_id: ThreadId) -> CodexResult<V2ResidencyPin> {
        self.residency.commit_slot(thread_id);
        self.active = false;
        self.residency.pin_required(thread_id)
    }
}

impl Drop for V2ResidencySlot {
    fn drop(&mut self) {
        if self.active {
            self.residency.release_pending_slot();
        }
    }
}

impl AgentControl {
    pub(super) async fn reserve_v2_residency_slot(
        &self,
        state: &Arc<ThreadManagerState>,
        config: &Config,
        protected_thread_id: Option<ThreadId>,
    ) -> CodexResult<V2ResidencySlot> {
        let capacity = config
            .effective_agent_max_threads(MultiAgentVersion::V2)
            .unwrap_or(usize::MAX);
        Arc::clone(&self.v2_residency)
            .reserve_slot(self, state, capacity, protected_thread_id)
            .await
    }

    pub(super) async fn touch_loaded_v2_residency(
        &self,
        state: &Arc<ThreadManagerState>,
        thread_id: ThreadId,
    ) {
        if let Ok(thread) = state.get_thread(thread_id).await
            && is_resident_candidate(thread.as_ref())
        {
            self.v2_residency.touch(thread_id);
        }
    }

    pub(super) fn forget_v2_residency(&self, thread_id: ThreadId) {
        self.v2_residency.remove(thread_id);
    }

    pub(super) fn pin_v2_residency(
        &self,
        thread_id: ThreadId,
    ) -> CodexResult<Option<V2ResidencyPin>> {
        self.v2_residency.pin_if_resident(thread_id)
    }
}

impl V2Residency {
    async fn reserve_slot(
        self: Arc<Self>,
        agent_control: &AgentControl,
        manager: &Arc<ThreadManagerState>,
        capacity: usize,
        protected_thread_id: Option<ThreadId>,
    ) -> CodexResult<V2ResidencySlot> {
        loop {
            if self.try_reserve_pending_slot(capacity) {
                return Ok(V2ResidencySlot {
                    residency: self,
                    active: true,
                });
            }
            if !self
                .try_unload_one_resident(agent_control, manager, protected_thread_id)
                .await
            {
                return Err(CodexErr::new(CodexErrorDetails::AgentLimitReached {
                    max_threads: capacity,
                }));
            }
        }
    }

    fn try_reserve_pending_slot(&self, capacity: usize) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.residents.len().saturating_add(state.pending_slots) >= capacity {
            return false;
        }
        state.pending_slots += 1;
        true
    }

    async fn try_unload_one_resident(
        &self,
        agent_control: &AgentControl,
        manager: &Arc<ThreadManagerState>,
        protected_thread_id: Option<ThreadId>,
    ) -> bool {
        let candidates_to_scan = self.resident_count();
        for _ in 0..candidates_to_scan {
            let Some(candidate_thread_id) = self.begin_eviction(protected_thread_id) else {
                return false;
            };
            let _lifecycle_guard = match agent_control
                .acquire_agent_lifecycle(candidate_thread_id)
                .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    self.cancel_eviction(candidate_thread_id);
                    continue;
                }
            };
            let Some(candidate_thread) = manager
                .get_thread(candidate_thread_id)
                .await
                .ok()
                .filter(|thread| is_resident_candidate(thread))
            else {
                self.commit_eviction(candidate_thread_id);
                continue;
            };
            if !is_unloadable(candidate_thread.as_ref()).await {
                self.cancel_eviction(candidate_thread_id);
                continue;
            }
            candidate_thread.ensure_rollout_materialized().await;
            if let Err(err) = candidate_thread.shutdown_and_wait().await {
                warn!(
                    "failed to shut down v2 resident thread before unloading {candidate_thread_id}: {err}"
                );
                self.cancel_eviction(candidate_thread_id);
                continue;
            }
            let _ = manager.remove_thread(&candidate_thread_id).await;
            self.commit_eviction(candidate_thread_id);
            return true;
        }
        false
    }

    fn resident_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .residents
            .len()
    }

    fn begin_eviction(&self, protected_thread_id: Option<ThreadId>) -> Option<ThreadId> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let candidate = state.residents.iter().copied().find(|thread_id| {
            Some(*thread_id) != protected_thread_id
                && !state.evicting.contains(thread_id)
                && state.pins.get(thread_id).copied().unwrap_or_default() == 0
        })?;
        state.evicting.insert(candidate);
        Some(candidate)
    }

    fn touch(&self, thread_id: ThreadId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.evicting.contains(&thread_id) {
            touch_resident(&mut state.residents, thread_id);
        }
    }

    fn remove(&self, thread_id: ThreadId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .residents
            .retain(|resident_thread_id| *resident_thread_id != thread_id);
        state.pins.remove(&thread_id);
        state.evicting.remove(&thread_id);
    }

    fn commit_slot(&self, thread_id: ThreadId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending_slots = state.pending_slots.saturating_sub(1);
        touch_resident(&mut state.residents, thread_id);
    }

    fn release_pending_slot(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending_slots = state.pending_slots.saturating_sub(1);
    }

    fn pin_if_resident(
        self: &Arc<Self>,
        thread_id: ThreadId,
    ) -> CodexResult<Option<V2ResidencyPin>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.evicting.contains(&thread_id) {
            return Err(CodexErr::UnsupportedOperation(format!(
                "agent `{thread_id}` is being unloaded; retry the operation"
            )));
        }
        if !state.residents.contains(&thread_id) {
            return Ok(None);
        }
        *state.pins.entry(thread_id).or_default() += 1;
        Ok(Some(V2ResidencyPin {
            residency: Arc::clone(self),
            thread_id,
        }))
    }

    fn pin_required(self: &Arc<Self>, thread_id: ThreadId) -> CodexResult<V2ResidencyPin> {
        self.pin_if_resident(thread_id)?.ok_or_else(|| {
            CodexErr::UnsupportedOperation(format!(
                "agent `{thread_id}` is not yet resident; retry the operation"
            ))
        })
    }

    fn unpin(&self, thread_id: ThreadId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(count) = state.pins.get_mut(&thread_id) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            state.pins.remove(&thread_id);
        }
    }

    fn commit_eviction(&self, thread_id: ThreadId) {
        self.remove(thread_id);
    }

    fn cancel_eviction(&self, thread_id: ThreadId) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.evicting.remove(&thread_id) {
            return;
        }
        touch_resident(&mut state.residents, thread_id);
    }
}

fn touch_resident(residents: &mut VecDeque<ThreadId>, thread_id: ThreadId) {
    residents.retain(|resident_thread_id| *resident_thread_id != thread_id);
    residents.push_back(thread_id);
}

fn is_resident_candidate(thread: &CodexThread) -> bool {
    thread.multi_agent_version() == Some(MultiAgentVersion::V2)
        && is_v2_resident_session_source(&thread.session_source)
}

pub(super) fn is_v2_resident_session_source(session_source: &SessionSource) -> bool {
    matches!(session_source, SessionSource::SubAgent(_))
}

async fn is_unloadable(thread: &CodexThread) -> bool {
    matches!(
        thread.agent_status().await,
        AgentStatus::Completed(_) | AgentStatus::Errored(_) | AgentStatus::Interrupted
    ) && thread.session.active_turn.lock().await.is_none()
        && !thread.session.input_queue.has_pending_mailbox_items().await
}

#[cfg(test)]
#[path = "residency_tests.rs"]
mod tests;
