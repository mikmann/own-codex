use super::*;
#[cfg(test)]
use codex_protocol::error::CodexErrorDetails;
use std::time::Duration;

const AGENT_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

impl AgentControl {
    /// Submit a shutdown request for a live agent without marking it explicitly closed in
    /// persisted spawn-edge state.
    #[cfg(test)]
    pub(crate) async fn shutdown_live_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.upgrade()?;
        let result = if let Ok(thread) = state.get_thread(agent_id).await {
            thread.session.ensure_rollout_materialized().await;
            thread.session.flush_rollout().await?;
            let result = if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state
                    .send_op(agent_id, Op::Shutdown {}, /*parent_turn_id*/ None)
                    .await
            };
            thread.wait_until_terminated().await;
            result
        } else {
            state
                .send_op(agent_id, Op::Shutdown {}, /*parent_turn_id*/ None)
                .await
        };
        let _ = state.remove_thread(&agent_id).await;
        self.forget_v2_residency(agent_id);
        self.state.release_spawned_thread(agent_id);
        result
    }

    #[cfg(test)]
    pub(crate) async fn shutdown_agent_tree(&self, agent_id: ThreadId) -> CodexResult<String> {
        let descendant_ids = self.live_thread_spawn_descendants(agent_id).await?;
        let result = self.shutdown_live_agent(agent_id).await;
        for descendant_id in descendant_ids {
            match self.shutdown_live_agent(descendant_id).await {
                Ok(_) => {}
                Err(error) if matches!(error.details(), CodexErrorDetails::ThreadNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        result
    }

    pub(super) async fn shutdown_live_agent_bounded(&self, agent_id: ThreadId) -> CodexResult<()> {
        let state = self.upgrade()?;
        let Ok(thread) = state.get_thread(agent_id).await else {
            self.forget_v2_residency(agent_id);
            self.state.release_spawned_thread(agent_id);
            return Ok(());
        };
        let close = async {
            thread.session.ensure_rollout_materialized().await;
            thread.session.flush_rollout().await?;
            let result = if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state
                    .send_op(agent_id, Op::Shutdown {}, /*parent_turn_id*/ None)
                    .await
            };
            thread.wait_until_terminated().await;
            result?;
            let _ = state.remove_thread(&agent_id).await;
            self.forget_v2_residency(agent_id);
            self.state.release_spawned_thread(agent_id);
            CodexResult::Ok(())
        };
        tokio::time::timeout(AGENT_CLOSE_TIMEOUT, close)
            .await
            .map_err(|_| {
                CodexErr::Fatal(format!(
                    "timed out while closing agent `{agent_id}` after {} seconds",
                    AGENT_CLOSE_TIMEOUT.as_secs()
                ))
            })?
    }
}
